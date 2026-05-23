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
    };

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
    run_slide_command(slide, &status_bar);
    Ok(())
}

fn run_slide_command(slide: &SlideCommand, status_bar: &str) {
    // Slides run attached directly to the terminal: no stdout/stderr capture, no ratatui
    // rendering layer. This lets terminal image protocols (imgcat, sixel, kitty graphics)
    // and arbitrary interactive programs work normally.
    let _ = disable_raw_mode();

    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let status = Command::new(shell)
        .arg("-lc")
        .arg(slide_shell_script(slide, status_bar))
        .current_dir(slide_command_cwd(slide))
        .status();

    if let Err(err) = status {
        eprintln!("failed to execute command: {err}");
    }
}

fn slide_shell_script(slide: &SlideCommand, status_bar: &str) -> String {
    format!(
        "clear\nprintf '%s\\n\\n\\n' {}\n{}",
        shell_single_quote(status_bar),
        slide.command
    )
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
    run_prompted_shell(&shell).context("failed to launch temporary shell")?;

    // Do not re-run the current slide after shell exit; just restore navigation hints.
    render_status_bar(app)
}

fn run_prompted_shell(shell: &str) -> io::Result<std::process::ExitStatus> {
    match shell_name(shell).as_deref() {
        Some("bash") => run_bash_with_tuition_prompt(shell),
        Some("zsh") => run_zsh_with_tuition_prompt(shell),
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

fn run_bash_with_tuition_prompt(shell: &str) -> io::Result<std::process::ExitStatus> {
    let rcfile = temporary_prompt_path("bashrc");
    let contents = r#"if [ -r "$HOME/.bashrc" ]; then
  . "$HOME/.bashrc"
fi
PS1='(TUITION) '
PROMPT_COMMAND='PS1="(TUITION) "'
"#;
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

fn run_zsh_with_tuition_prompt(shell: &str) -> io::Result<std::process::ExitStatus> {
    let zdotdir = temporary_prompt_path("zsh");
    fs::create_dir_all(&zdotdir)?;
    let contents = r#"if [ -n "$TUITION_ORIGINAL_ZDOTDIR" ] && [ -r "$TUITION_ORIGINAL_ZDOTDIR/.zshrc" ]; then
  . "$TUITION_ORIGINAL_ZDOTDIR/.zshrc"
elif [ -r "$HOME/.zshrc" ]; then
  . "$HOME/.zshrc"
fi
PROMPT='(TUITION) '
PS1='(TUITION) '
RPROMPT=''
RPS1=''
precmd_functions=(tuition_prompt_precmd)
tuition_prompt_precmd() {
  PROMPT='(TUITION) '
  PS1='(TUITION) '
  RPROMPT=''
  RPS1=''
}
"#;
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
        let commands = parse_slide_files(&[path.clone()]).unwrap();

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
    fn slide_shell_script_starts_with_clear() {
        let slide = SlideCommand {
            file: PathBuf::from("test"),
            line: 1,
            command: "printf hello".to_string(),
        };

        assert_eq!(
            slide_shell_script(&slide, "status"),
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

    fn temp_file(name: &str, contents: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = env::temp_dir().join(format!("tuition-{name}-{unique}.txt"));
        fs::write(&path, contents).unwrap();
        path
    }
}
