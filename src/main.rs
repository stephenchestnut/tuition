use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode},
};

#[derive(Debug, Parser)]
#[command(author, version, about)]
struct Cli {
    /// Source these aliases files before running slides and temporary shells.
    /// Separate multiple files with semicolons, for example: --aliases './aliases;./more-aliases'.
    #[arg(long)]
    aliases: Option<String>,

    /// Print and run just one slide, then exit.
    #[arg(long)]
    slide: Option<usize>,

    /// Files containing slide commands, one command per non-blank non-comment line.
    #[arg(required = true)]
    files: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SlideCommand {
    file: PathBuf,
    line: usize,
    command: String,
}

#[derive(Debug, Clone)]
struct ActiveSlide {
    index: usize,
}

#[derive(Debug)]
struct App {
    commands: Vec<SlideCommand>,
    active: ActiveSlide,
    aliases: Vec<PathBuf>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let commands = parse_slide_files(&cli.files)?;
    if commands.is_empty() {
        return Err(anyhow!("no slide commands found"));
    }

    let mut app = App {
        commands,
        active: ActiveSlide { index: 0 },
        aliases: parse_aliases_arg(cli.aliases.as_deref())?,
    };

    if let Some(slide_number) = cli.slide {
        let result = run_single_slide(&mut app, slide_number);
        let cleanup_result = cleanup_terminal();
        return result.and(cleanup_result);
    }

    let result = run_app(&mut app);
    let cleanup_result = cleanup_terminal();

    result.and(cleanup_result)
}

fn parse_slide_files(files: &[PathBuf]) -> Result<Vec<SlideCommand>> {
    let mut commands = Vec::new();

    for file in files {
        let contents = fs::read_to_string(file)
            .with_context(|| format!("failed to read slide file {}", file.display()))?;

        for (idx, line) in contents.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            commands.push(SlideCommand {
                file: file.clone(),
                line: idx + 1,
                command: line.to_string(),
            });
        }
    }

    Ok(commands)
}

fn run_app(app: &mut App) -> Result<()> {
    render_active_slide(app)?;

    loop {
        enable_raw_mode().context("failed to enable raw mode")?;
        let key = read_key();
        disable_raw_mode().context("failed to disable raw mode")?;
        let key = key?;

        if handle_key(app, key)? {
            return Ok(());
        }
    }
}

fn run_single_slide(app: &mut App, slide_number: usize) -> Result<()> {
    if slide_number == 0 {
        return Err(anyhow!("--slide must be at least 1"));
    }

    let index = slide_number - 1;
    if index >= app.commands.len() {
        return Err(anyhow!(
            "--slide {} is out of range; there are {} slides",
            slide_number,
            app.commands.len()
        ));
    }

    app.active.index = index;
    render_active_slide(app)
}

fn read_key() -> Result<KeyEvent> {
    loop {
        if let Event::Key(key) = event::read().context("failed to read terminal event")? {
            return Ok(key);
        }
    }
}

fn handle_key(app: &mut App, key: KeyEvent) -> Result<bool> {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Ok(true);
    }

    match key.code {
        KeyCode::Char('l') | KeyCode::Right | KeyCode::Char(' ') => next_slide(app)?,
        KeyCode::Char('h') | KeyCode::Left => previous_slide(app)?,
        KeyCode::Char('r') => render_active_slide(app)?,
        KeyCode::Char('s') => open_temporary_shell(app)?,
        KeyCode::Char('q') => return confirm_quit(),
        _ => {}
    }

    Ok(false)
}

fn confirm_quit() -> Result<bool> {
    print!("\nQuit tuition? y/n ");
    io::stdout().flush().ok();

    enable_raw_mode().context("failed to enable raw mode")?;
    let key = read_key();
    disable_raw_mode().context("failed to disable raw mode")?;

    match key?.code {
        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter | KeyCode::Char('q') => Ok(true),
        _ => {
            println!("cancelled");
            Ok(false)
        }
    }
}

fn next_slide(app: &mut App) -> Result<()> {
    let next = app.active.index + 1;
    if next < app.commands.len() {
        activate_slide(app, next)?;
    }
    Ok(())
}

fn previous_slide(app: &mut App) -> Result<()> {
    if app.active.index > 0 {
        activate_slide(app, app.active.index - 1)?;
    }
    Ok(())
}

fn activate_slide(app: &mut App, index: usize) -> Result<()> {
    app.active.index = index;
    render_active_slide(app)
}

fn render_active_slide(app: &mut App) -> Result<()> {
    let slide = &app.commands[app.active.index];
    let status_bar = status_bar(app);
    run_slide_command(slide, &status_bar, &app.aliases);
    Ok(())
}

fn run_slide_command(slide: &SlideCommand, status_bar: &str, aliases_paths: &[PathBuf]) {
    // Slides run attached directly to the terminal: no stdout/stderr capture, no ratatui
    // rendering layer. This lets terminal image protocols (imgcat, sixel, kitty graphics)
    // and arbitrary interactive programs work normally.
    let _ = disable_raw_mode();

    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let shell_name = shell_name(&shell);
    let status = Command::new(shell)
        .arg("-lc")
        .arg(slide_shell_script(
            slide,
            status_bar,
            shell_name.as_deref(),
            aliases_paths,
        ))
        .current_dir(slide_command_cwd(slide))
        .status();

    if let Err(err) = status {
        eprintln!("failed to execute command: {err}");
    }
}

fn slide_shell_script(
    slide: &SlideCommand,
    status_bar: &str,
    shell_name: Option<&str>,
    aliases_paths: &[PathBuf],
) -> String {
    let mut script = String::from("clear\n");

    // zsh does not split unquoted scalar expansions by default, but the slide
    // files are written as shell snippets and expect POSIX-style word splitting
    // (for example: `for name in $row`).
    if matches!(shell_name, Some("zsh")) {
        script.push_str("emulate -L sh\n");
    }

    if let Some(prelude) = aliases_prelude(shell_name, aliases_paths, &slide_command_cwd(slide)) {
        script.push_str(&prelude);
    }

    script.push_str(&format!(
        "printf '%s\\n\\n\\n' {}\n{}",
        shell_single_quote(status_bar),
        slide.command
    ));
    script
}

fn shell_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn slide_command_cwd(slide: &SlideCommand) -> PathBuf {
    slide
        .file
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}

fn parse_aliases_arg(aliases: Option<&str>) -> Result<Vec<PathBuf>> {
    aliases
        .into_iter()
        .flat_map(|aliases| aliases.split(';'))
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(|path| absolutize_path(Path::new(path)))
        .collect()
}

fn absolutize_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(env::current_dir()
            .context("failed to determine current directory")?
            .join(path))
    }
}

fn aliases_prelude(
    shell_name: Option<&str>,
    aliases_paths: &[PathBuf],
    start_dir: &Path,
) -> Option<String> {
    let aliases = effective_aliases_paths(aliases_paths, start_dir)?;

    let mut prelude = String::new();
    if matches!(shell_name, Some("bash")) {
        prelude.push_str("shopt -s expand_aliases\n");
    }
    for aliases_path in aliases {
        let quoted_path = shell_single_quote(&aliases_path.to_string_lossy());
        prelude.push_str(&format!(
            "if [ -r {} ]; then\n  . {}\nfi\n",
            quoted_path, quoted_path
        ));
        prelude.push_str(&aliases_function_definitions(&aliases_path));
    }
    Some(prelude)
}

fn effective_aliases_paths(aliases_paths: &[PathBuf], start_dir: &Path) -> Option<Vec<PathBuf>> {
    if aliases_paths.is_empty() {
        find_aliases_file(start_dir).map(|path| vec![path])
    } else {
        Some(aliases_paths.to_vec())
    }
}

fn aliases_function_definitions(path: &Path) -> String {
    let Ok(contents) = fs::read_to_string(path) else {
        return String::new();
    };

    let mut defs = String::new();
    for line in contents.lines() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("alias ") else {
            continue;
        };
        let Some((name, value)) = rest.split_once('=') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() || !is_shell_name(name) {
            continue;
        }

        let body = strip_matching_quotes(value.trim());
        defs.push_str(name);
        defs.push_str("() { ");
        defs.push_str(body);
        defs.push_str("; }\n");
    }
    defs
}

fn strip_matching_quotes(s: &str) -> &str {
    if s.len() >= 2 {
        let bytes = s.as_bytes();
        let first = bytes[0];
        let last = bytes[s.len() - 1];
        if (first == b'\'' || first == b'\"') && first == last {
            return &s[1..s.len() - 1];
        }
    }
    s
}

fn is_shell_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn find_aliases_file(start_dir: &Path) -> Option<PathBuf> {
    for dir in start_dir.ancestors() {
        let candidate = dir.join("aliases");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn status_bar(app: &App) -> String {
    let slide = &app.commands[app.active.index];
    format!(
        "\x1b[7m {}/{} \x1b[0m {}:{} | (r)erun | (s)hell | (q)uit",
        app.active.index + 1,
        app.commands.len(),
        slide.file.display(),
        slide.line,
    )
}

fn render_status_bar(app: &App) -> Result<()> {
    println!("{}", status_bar(app));
    io::stdout().flush().context("failed to flush stdout")
}

fn cleanup_terminal() -> Result<()> {
    let _ = disable_raw_mode();
    Ok(())
}

fn open_temporary_shell(app: &App) -> Result<()> {
    let _ = disable_raw_mode();

    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    run_prompted_shell(&shell, &app.aliases).context("failed to launch temporary shell")?;

    // Do not re-run the current slide after shell exit; just restore navigation hints.
    render_status_bar(app)
}

fn run_prompted_shell(
    shell: &str,
    aliases_paths: &[PathBuf],
) -> io::Result<std::process::ExitStatus> {
    let aliases = env::current_dir()
        .ok()
        .and_then(|dir| effective_aliases_paths(aliases_paths, &dir))
        .unwrap_or_default();
    match shell_name(shell).as_deref() {
        Some("bash") => run_bash_with_tuition_prompt(shell, &aliases),
        Some("zsh") => run_zsh_with_tuition_prompt(shell, &aliases),
        Some("fish") => run_fish_with_tuition_prompt(shell),
        _ => Command::new(shell)
            .env("PS1", "(TUITION) ")
            .env("PROMPT", "(TUITION) ")
            .status(),
    }
}

fn shell_name(shell: &str) -> Option<String> {
    Path::new(shell)
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.trim_start_matches('-').to_string())
}

fn run_bash_with_tuition_prompt(
    shell: &str,
    aliases_paths: &[PathBuf],
) -> io::Result<std::process::ExitStatus> {
    let rcfile = temporary_prompt_path("bashrc");
    let mut contents = String::from(
        r#"if [ -r "$HOME/.bashrc" ]; then
  . "$HOME/.bashrc"
fi
"#,
    );
    if !aliases_paths.is_empty() {
        contents.push_str("shopt -s expand_aliases\n");
    }
    for aliases_path in aliases_paths {
        let aliases_path = shell_single_quote(&aliases_path.to_string_lossy());
        contents.push_str(&format!(
            "if [ -r {} ]; then\n  . {}\nfi\n",
            aliases_path, aliases_path
        ));
    }
    contents.push_str("PS1='(TUITION) '\nPROMPT_COMMAND='PS1=\"(TUITION) \"'\n");
    fs::write(&rcfile, contents)?;
    let result = Command::new(shell)
        .arg("--rcfile")
        .arg(&rcfile)
        .arg("-i")
        .env("PS1", "(TUITION) ")
        .status();
    let _ = fs::remove_file(rcfile);
    result
}

fn run_zsh_with_tuition_prompt(
    shell: &str,
    aliases_paths: &[PathBuf],
) -> io::Result<std::process::ExitStatus> {
    let zdotdir = temporary_prompt_path("zsh");
    fs::create_dir_all(&zdotdir)?;
    let mut contents = String::from(
        r#"if [ -n "$TUITION_ORIGINAL_ZDOTDIR" ] && [ -r "$TUITION_ORIGINAL_ZDOTDIR/.zshrc" ]; then
  . "$TUITION_ORIGINAL_ZDOTDIR/.zshrc"
elif [ -r "$HOME/.zshrc" ]; then
  . "$HOME/.zshrc"
fi
"#,
    );
    for aliases_path in aliases_paths {
        let aliases_path = shell_single_quote(&aliases_path.to_string_lossy());
        contents.push_str(&format!(
            "if [ -r {} ]; then\n  . {}\nfi\n",
            aliases_path, aliases_path
        ));
    }
    contents.push_str(
        "PROMPT='(TUITION) '\nPS1='(TUITION) '\nRPROMPT=''\nRPS1=''\nprecmd_functions=(tuition_prompt_precmd)\ntuition_prompt_precmd() {\n  PROMPT='(TUITION) '\n  PS1='(TUITION) '\n  RPROMPT=''\n  RPS1=''\n}\n",
    );
    fs::write(zdotdir.join(".zshrc"), contents)?;
    let mut command = Command::new(shell);
    command
        .env("ZDOTDIR", &zdotdir)
        .env("PS1", "(TUITION) ")
        .env("PROMPT", "(TUITION) ");
    if let Ok(original_zdotdir) = env::var("ZDOTDIR") {
        command.env("TUITION_ORIGINAL_ZDOTDIR", original_zdotdir);
    }
    let result = command.status();
    let _ = fs::remove_dir_all(zdotdir);
    result
}

fn run_fish_with_tuition_prompt(shell: &str) -> io::Result<std::process::ExitStatus> {
    let config_home = temporary_prompt_path("fish");
    let fish_dir = config_home.join("fish");
    fs::create_dir_all(&fish_dir)?;
    let contents = r#"if test -r "$TUITION_ORIGINAL_FISH_CONFIG"
  source "$TUITION_ORIGINAL_FISH_CONFIG"
end
function fish_prompt
  echo -n '(TUITION) '
end
"#;
    fs::write(fish_dir.join("config.fish"), contents)?;
    let original_config = env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env::var("HOME").unwrap_or_default()).join(".config"))
        .join("fish/config.fish");
    let result = Command::new(shell)
        .env("XDG_CONFIG_HOME", &config_home)
        .env("TUITION_ORIGINAL_FISH_CONFIG", original_config)
        .status();
    let _ = fs::remove_dir_all(config_home);
    result
}

fn temporary_prompt_path(kind: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    env::temp_dir().join(format!("tuition-{kind}-{}-{nanos}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parser_ignores_blank_and_comment_lines() {
        let path = temp_file(
            "slides",
            "\n# comment\n echo one\n\t# indented comment\necho two\n",
        );
        let commands = parse_slide_files(std::slice::from_ref(&path)).unwrap();

        assert_eq!(
            commands,
            vec![
                SlideCommand {
                    file: path.clone(),
                    line: 3,
                    command: " echo one".to_string()
                },
                SlideCommand {
                    file: path,
                    line: 5,
                    command: "echo two".to_string()
                },
            ]
        );
    }

    #[test]
    fn parser_preserves_multiple_file_order() {
        let first = temp_file("slides-a", "echo a1\necho a2\n");
        let second = temp_file("slides-b", "echo b1\n");
        let commands = parse_slide_files(&[first.clone(), second.clone()]).unwrap();

        assert_eq!(commands[0].file, first);
        assert_eq!(commands[0].command, "echo a1");
        assert_eq!(commands[1].command, "echo a2");
        assert_eq!(commands[2].file, second);
        assert_eq!(commands[2].command, "echo b1");
    }

    #[test]
    fn parser_missing_file_returns_contextual_error() {
        let missing = unique_temp_path("missing-slides");
        let err = parse_slide_files(std::slice::from_ref(&missing))
            .unwrap_err()
            .to_string();

        assert!(err.contains("failed to read slide file"));
        assert!(err.contains(&missing.display().to_string()));
    }

    #[test]
    fn parser_ignores_whitespace_only_lines() {
        let path = temp_file("slides", "   \n\t\n echo one\n");
        let commands = parse_slide_files(&[path]).unwrap();

        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].line, 3);
        assert_eq!(commands[0].command, " echo one");
    }

    #[test]
    fn parser_ignores_indented_comments() {
        let path = temp_file("slides", "  # comment\necho one\n");
        let commands = parse_slide_files(&[path]).unwrap();

        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].line, 2);
    }

    #[test]
    fn parser_preserves_inline_hashes() {
        let path = temp_file("slides", "echo before # after\n");
        let commands = parse_slide_files(&[path]).unwrap();

        assert_eq!(commands[0].command, "echo before # after");
    }

    #[test]
    fn run_single_slide_rejects_zero() {
        let mut app = test_app(vec![":"], vec![]);
        let err = run_single_slide(&mut app, 0).unwrap_err().to_string();

        assert!(err.contains("--slide must be at least 1"));
    }

    #[test]
    fn run_single_slide_rejects_out_of_range() {
        let mut app = test_app(vec![":"], vec![]);
        let err = run_single_slide(&mut app, 2).unwrap_err().to_string();

        assert!(err.contains("--slide 2 is out of range; there are 1 slides"));
    }

    #[test]
    fn slide_shell_script_starts_with_clear() {
        let slide = SlideCommand {
            file: PathBuf::from("/tmp/tuition/test.txt"),
            line: 1,
            command: "printf hello".to_string(),
        };

        assert_eq!(
            slide_shell_script(&slide, "status", Some("bash"), &[]),
            "clear\nprintf '%s\\n\\n\\n' 'status'\nprintf hello"
        );
    }

    #[test]
    fn slide_command_cwd_uses_slide_file_directory() {
        let slide = SlideCommand {
            file: PathBuf::from("/tmp/tuition/slides.txt"),
            line: 1,
            command: "pwd".to_string(),
        };

        assert_eq!(slide_command_cwd(&slide), PathBuf::from("/tmp/tuition"));
    }

    #[test]
    fn slide_command_cwd_returns_dot_for_files_without_parent_directory() {
        let slide = SlideCommand {
            file: PathBuf::from("slides.txt"),
            line: 1,
            command: "pwd".to_string(),
        };

        assert_eq!(slide_command_cwd(&slide), PathBuf::from("."));
    }

    #[test]
    fn shell_single_quote_handles_apostrophes() {
        assert_eq!(shell_single_quote("don't"), "'don'\\''t'");
    }

    #[test]
    fn zsh_scripts_include_emulate_sh() {
        let slide = test_slide("echo hi");
        let script = slide_shell_script(&slide, "status", Some("zsh"), &[]);

        assert!(script.contains("emulate -L sh\n"));
    }

    #[test]
    fn bash_alias_prelude_includes_expand_aliases() {
        let aliases = unique_temp_path("aliases");
        let prelude = aliases_prelude(Some("bash"), &[aliases], Path::new(".")).unwrap();

        assert!(prelude.contains("shopt -s expand_aliases"));
    }

    #[test]
    fn status_bar_includes_count_file_line_and_hints() {
        let app = test_app(vec!["echo one", "echo two"], vec![]);
        let status = status_bar(&app);

        assert!(status.contains("1/2"));
        assert!(status.contains("slides.txt:1"));
        assert!(status.contains("(r)erun"));
        assert!(status.contains("(s)hell"));
        assert!(status.contains("(q)uit"));
    }

    #[test]
    fn generated_slide_script_includes_status_bar_before_command() {
        let slide = test_slide("echo command");
        let script = slide_shell_script(&slide, "STATUS", Some("sh"), &[]);

        let status_pos = script.find("STATUS").unwrap();
        let command_pos = script.find("echo command").unwrap();
        assert!(status_pos < command_pos);
    }

    #[test]
    fn parses_semicolon_separated_aliases_arg() {
        let aliases = parse_aliases_arg(Some("aliases; examples/more-aliases ;")).unwrap();
        let cwd = env::current_dir().unwrap();

        assert_eq!(
            aliases,
            vec![cwd.join("aliases"), cwd.join("examples/more-aliases")]
        );
    }

    #[test]
    fn empty_alias_arg_segments_are_ignored() {
        let aliases = parse_aliases_arg(Some("; aliases ;; ;")).unwrap();
        let cwd = env::current_dir().unwrap();

        assert_eq!(aliases, vec![cwd.join("aliases")]);
    }

    #[test]
    fn absolute_alias_paths_remain_absolute() {
        let absolute = unique_temp_path("aliases");
        let aliases = parse_aliases_arg(Some(&absolute.to_string_lossy())).unwrap();

        assert_eq!(aliases, vec![absolute]);
    }

    #[test]
    fn aliases_function_definitions_turn_aliases_into_functions() {
        let path = temp_file(
            "aliases",
            "alias el=\"printf \\\"\\n\\n\\\"\"\nalias s4=\"printf \\\"    \\\"\"\nexport FOO=bar\n",
        );

        let defs = aliases_function_definitions(&path);
        assert!(defs.contains("el() { printf \\\"\\n\\n\\\"; }"));
        assert!(defs.contains("s4() { printf \\\"    \\\"; }"));
    }

    #[test]
    fn invalid_alias_names_are_ignored() {
        let path = temp_file("aliases", "alias 1bad='echo no'\nalias good='echo yes'\n");
        let defs = aliases_function_definitions(&path);

        assert!(!defs.contains("1bad()"));
        assert!(defs.contains("good() { echo yes; }"));
    }

    #[test]
    fn quoted_alias_bodies_are_stripped() {
        let path = temp_file(
            "aliases",
            "alias sq='echo single'\nalias dq=\"echo double\"\n",
        );
        let defs = aliases_function_definitions(&path);

        assert!(defs.contains("sq() { echo single; }"));
        assert!(defs.contains("dq() { echo double; }"));
    }

    #[test]
    fn missing_alias_file_returns_empty_function_definitions() {
        let missing = unique_temp_path("missing-aliases");

        assert_eq!(aliases_function_definitions(&missing), "");
    }

    #[test]
    fn find_aliases_file_finds_nearest_upward_aliases() {
        let root = temp_dir("aliases-root");
        let child = root.join("child");
        let grandchild = child.join("grandchild");
        fs::create_dir_all(&grandchild).unwrap();
        fs::write(root.join("aliases"), "alias root='echo root'\n").unwrap();
        fs::write(child.join("aliases"), "alias child='echo child'\n").unwrap();

        assert_eq!(find_aliases_file(&grandchild), Some(child.join("aliases")));
    }

    #[test]
    fn next_slide_increments_when_not_at_final_slide() {
        let mut app = test_app(vec![":", ":"], vec![unique_temp_path("aliases")]);
        next_slide(&mut app).unwrap();

        assert_eq!(app.active.index, 1);
    }

    #[test]
    fn next_slide_does_nothing_at_final_slide() {
        let mut app = test_app(vec![":"], vec![unique_temp_path("aliases")]);
        next_slide(&mut app).unwrap();

        assert_eq!(app.active.index, 0);
    }

    #[test]
    fn previous_slide_decrements_when_not_at_first_slide() {
        let mut app = test_app(vec![":", ":"], vec![unique_temp_path("aliases")]);
        app.active.index = 1;
        previous_slide(&mut app).unwrap();

        assert_eq!(app.active.index, 0);
    }

    #[test]
    fn previous_slide_does_nothing_at_first_slide() {
        let mut app = test_app(vec![":"], vec![unique_temp_path("aliases")]);
        previous_slide(&mut app).unwrap();

        assert_eq!(app.active.index, 0);
    }

    fn test_app(commands: Vec<&str>, aliases: Vec<PathBuf>) -> App {
        App {
            commands: commands
                .into_iter()
                .enumerate()
                .map(|(idx, command)| SlideCommand {
                    file: PathBuf::from("slides.txt"),
                    line: idx + 1,
                    command: command.to_string(),
                })
                .collect(),
            active: ActiveSlide { index: 0 },
            aliases,
        }
    }

    fn test_slide(command: &str) -> SlideCommand {
        SlideCommand {
            file: PathBuf::from("slides.txt"),
            line: 1,
            command: command.to_string(),
        }
    }

    fn temp_file(name: &str, contents: &str) -> PathBuf {
        let path = unique_temp_path(name);
        fs::write(&path, contents).unwrap();
        path
    }

    fn temp_dir(name: &str) -> PathBuf {
        let path = unique_temp_path(name);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn unique_temp_path(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        env::temp_dir().join(format!("tuition-{name}-{}-{unique}", std::process::id()))
    }
}
