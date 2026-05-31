use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Result;

use crate::{
    aliases::{aliases_prelude, effective_aliases_paths},
    app::CommandRunner,
    slides::{SlideCommand, slide_command_cwd},
};

pub struct ShellCommandRunner;

impl CommandRunner for ShellCommandRunner {
    fn run_slide(
        &mut self,
        slide: &SlideCommand,
        status_bar: &str,
        aliases: &[PathBuf],
    ) -> Result<()> {
        run_slide_command(slide, status_bar, aliases);
        Ok(())
    }

    fn open_temporary_shell(&mut self, aliases: &[PathBuf]) -> Result<()> {
        let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        run_prompted_shell(&shell, aliases)?;
        Ok(())
    }
}

pub fn run_slide_command(slide: &SlideCommand, status_bar: &str, aliases_paths: &[PathBuf]) {
    // Slides run attached directly to the terminal: no stdout/stderr capture, no ratatui
    // rendering layer. This lets terminal image protocols (imgcat, sixel, kitty graphics)
    // and arbitrary interactive programs work normally.
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

pub fn slide_shell_script(
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

pub fn slide_pdf_script(
    slide: &SlideCommand,
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

    script.push_str(&slide.command);
    script
}

pub fn shell_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub fn run_prompted_shell(
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

pub fn shell_name(shell: &str) -> Option<String> {
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
    fs::write(&rcfile, bash_prompt_rcfile_contents(aliases_paths))?;
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
    fs::write(
        zdotdir.join(".zshrc"),
        zsh_prompt_zshrc_contents(aliases_paths),
    )?;
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
    fs::write(fish_dir.join("config.fish"), fish_prompt_config_contents())?;
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

pub fn bash_prompt_rcfile_contents(aliases_paths: &[PathBuf]) -> String {
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
    contents
}

pub fn zsh_prompt_zshrc_contents(aliases_paths: &[PathBuf]) -> String {
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
    contents
}

pub fn fish_prompt_config_contents() -> String {
    r#"if test -r "$TUITION_ORIGINAL_FISH_CONFIG"
  source "$TUITION_ORIGINAL_FISH_CONFIG"
end
function fish_prompt
  echo -n '(TUITION) '
end
"#
    .to_string()
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

    #[test]
    fn slide_shell_script_starts_with_clear() {
        let slide = test_slide("printf hello");

        assert_eq!(
            slide_shell_script(&slide, "status", Some("bash"), &[]),
            "clear\nprintf '%s\\n\\n\\n' 'status'\nprintf hello"
        );
    }

    #[test]
    fn slide_pdf_script_starts_with_clear() {
        let slide = test_slide("printf hello");

        assert_eq!(
            slide_pdf_script(&slide, Some("bash"), &[]),
            "clear\nprintf hello"
        );
    }

    #[test]
    fn slide_pdf_script_omits_status_and_navigation_text() {
        let slide = test_slide("echo command");
        let script = slide_pdf_script(&slide, Some("sh"), &[]);

        assert!(!script.contains("STATUS"));
        assert!(!script.contains("(r)erun"));
        assert!(script.contains("echo command"));
    }

    #[test]
    fn slide_pdf_zsh_scripts_include_emulate_sh() {
        let slide = test_slide("echo hi");
        let script = slide_pdf_script(&slide, Some("zsh"), &[]);

        assert!(script.contains("emulate -L sh\n"));
    }

    #[test]
    fn slide_pdf_script_includes_aliases() {
        let slide = test_slide("ll");
        let script = slide_pdf_script(&slide, Some("bash"), &[PathBuf::from("/tmp/aliases")]);

        assert!(script.contains("shopt -s expand_aliases"));
        assert!(script.contains("/tmp/aliases"));
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
    fn generated_slide_script_includes_status_bar_before_command() {
        let slide = test_slide("echo command");
        let script = slide_shell_script(&slide, "STATUS", Some("sh"), &[]);

        let status_pos = script.find("STATUS").unwrap();
        let command_pos = script.find("echo command").unwrap();
        assert!(status_pos < command_pos);
    }

    #[test]
    fn bash_loads_home_bashrc_and_aliases() {
        let path = PathBuf::from("/tmp/tuition alias");
        let contents = bash_prompt_rcfile_contents(&[path]);

        assert!(contents.contains("$HOME/.bashrc"));
        assert!(contents.contains("shopt -s expand_aliases"));
        assert!(contents.contains("'/tmp/tuition alias'"));
    }

    #[test]
    fn bash_omits_alias_expansion_without_aliases() {
        let contents = bash_prompt_rcfile_contents(&[]);

        assert!(!contents.contains("shopt -s expand_aliases"));
    }

    #[test]
    fn zsh_loads_original_zshrc_and_quotes_aliases() {
        let path = PathBuf::from("/tmp/don't");
        let contents = zsh_prompt_zshrc_contents(&[path]);

        assert!(contents.contains("TUITION_ORIGINAL_ZDOTDIR"));
        assert!(contents.contains("'/tmp/don'\\''t'"));
    }

    #[test]
    fn fish_defines_tuition_prompt() {
        let contents = fish_prompt_config_contents();

        assert!(contents.contains("function fish_prompt"));
        assert!(contents.contains("(TUITION)"));
    }

    fn test_slide(command: &str) -> SlideCommand {
        SlideCommand {
            file: PathBuf::from("/tmp/tuition-test-slides/slides.txt"),
            line: 1,
            command: command.to_string(),
        }
    }
}
