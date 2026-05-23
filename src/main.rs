use std::{
    env, fs,
    io::{self, Stdout},
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use ansi_to_tui::IntoText;
use anyhow::{anyhow, Context, Result};
use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame, Terminal,
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
    output: SlideOutput,
    scroll: u16,
}

#[derive(Debug, Clone)]
struct SlideOutput {
    success: bool,
    code: Option<i32>,
    stdout: String,
    stderr: String,
    duration: Duration,
}

#[derive(Debug)]
struct App {
    commands: Vec<SlideCommand>,
    active: ActiveSlide,
    mode: Mode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Presenting,
    ConfirmQuit,
}

type Tui = Terminal<CrosstermBackend<Stdout>>;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let commands = parse_slide_files(&cli.files)?;
    if commands.is_empty() {
        return Err(anyhow!("no slide commands found"));
    }

    let first_output = run_slide_command(&commands[0]);
    let mut app = App {
        commands,
        active: ActiveSlide {
            index: 0,
            output: first_output,
            scroll: 0,
        },
        mode: Mode::Presenting,
    };

    let mut terminal = setup_tui()?;
    let result = run_app(&mut terminal, &mut app);
    let cleanup_result = cleanup_tui(&mut terminal);

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

fn run_slide_command(slide: &SlideCommand) -> SlideOutput {
    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let started = Instant::now();
    let output = Command::new(shell).arg("-lc").arg(&slide.command).output();
    let duration = started.elapsed();

    match output {
        Ok(output) => SlideOutput {
            success: output.status.success(),
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            duration,
        },
        Err(err) => SlideOutput {
            success: false,
            code: None,
            stdout: String::new(),
            stderr: format!("failed to execute command: {err}"),
            duration,
        },
    }
}

fn setup_tui() -> Result<Tui> {
    enable_raw_mode().context("failed to enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("failed to enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("failed to create terminal")?;
    terminal.clear().context("failed to clear terminal")?;
    Ok(terminal)
}

fn cleanup_tui(terminal: &mut Tui) -> Result<()> {
    disable_raw_mode().context("failed to disable raw mode")?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)
        .context("failed to leave alternate screen")?;
    terminal.show_cursor().context("failed to show cursor")?;
    Ok(())
}

fn run_app(terminal: &mut Tui, app: &mut App) -> Result<()> {
    loop {
        terminal.draw(|frame| render(frame, app))?;

        if let Event::Key(key) = event::read().context("failed to read terminal event")? {
            if handle_key(terminal, app, key)? {
                return Ok(());
            }
        }
    }
}

fn handle_key(terminal: &mut Tui, app: &mut App, key: KeyEvent) -> Result<bool> {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Ok(true);
    }

    match app.mode {
        Mode::Presenting => handle_presenting_key(terminal, app, key),
        Mode::ConfirmQuit => Ok(handle_confirm_quit_key(app, key)),
    }
}

fn handle_presenting_key(terminal: &mut Tui, app: &mut App, key: KeyEvent) -> Result<bool> {
    match key.code {
        KeyCode::Char('l') | KeyCode::Right | KeyCode::Char(' ') => next_slide(app),
        KeyCode::Char('h') | KeyCode::Left => previous_slide(app),
        KeyCode::Char('j') | KeyCode::Down => {
            app.active.scroll = app.active.scroll.saturating_add(1)
        }
        KeyCode::Char('k') | KeyCode::Up => app.active.scroll = app.active.scroll.saturating_sub(1),
        KeyCode::Char('q') => app.mode = Mode::ConfirmQuit,
        KeyCode::Char('s') => open_temporary_shell(terminal)?,
        _ => {}
    }

    Ok(false)
}

fn handle_confirm_quit_key(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter | KeyCode::Char('q') => true,
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.mode = Mode::Presenting;
            false
        }
        _ => false,
    }
}

fn next_slide(app: &mut App) {
    let next = app.active.index + 1;
    if next < app.commands.len() {
        activate_slide(app, next);
    }
}

fn previous_slide(app: &mut App) {
    if app.active.index > 0 {
        activate_slide(app, app.active.index - 1);
    }
}

fn activate_slide(app: &mut App, index: usize) {
    app.active.index = index;
    app.active.scroll = 0;
    app.active.output = run_slide_command(&app.commands[index]);
}

fn open_temporary_shell(terminal: &mut Tui) -> Result<()> {
    disable_raw_mode().context("failed to disable raw mode")?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)
        .context("failed to leave alternate screen")?;
    terminal.show_cursor().context("failed to show cursor")?;

    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let shell_result = run_prompted_shell(&shell);

    enable_raw_mode().context("failed to re-enable raw mode")?;
    execute!(terminal.backend_mut(), EnterAlternateScreen)
        .context("failed to re-enter alternate screen")?;
    terminal.clear().context("failed to clear terminal")?;

    shell_result.context("failed to launch temporary shell")?;
    Ok(())
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

fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);

    let slide = &app.commands[app.active.index];
    let text = if app.active.output.success {
        ansi_text(&app.active.output.stdout)
    } else {
        Text::from(diagnostic_slide(slide, &app.active.output))
    };

    let paragraph = Paragraph::new(text)
        .wrap(Wrap { trim: false })
        .scroll((app.active.scroll, 0));
    frame.render_widget(paragraph, chunks[0]);

    let footer = footer_line(app);
    frame.render_widget(Paragraph::new(footer), chunks[1]);

    if app.mode == Mode::ConfirmQuit {
        render_quit_confirmation(frame);
    }
}

fn ansi_text(stdout: &str) -> Text<'_> {
    stdout
        .into_text()
        .unwrap_or_else(|_| Text::from(strip_escape_fallback(stdout)))
}

fn strip_escape_fallback(s: &str) -> String {
    s.chars().filter(|ch| *ch != '\u{1b}').collect()
}

fn diagnostic_slide(slide: &SlideCommand, output: &SlideOutput) -> String {
    format!(
        "Command failed\n\nFile: {}\nLine: {}\nExit: {}\nDuration: {}\n\nCommand:\n{}\n\nSTDOUT:\n{}\n\nSTDERR:\n{}",
        slide.file.display(),
        slide.line,
        output
            .code
            .map(|code| code.to_string())
            .unwrap_or_else(|| "signal/unknown".to_string()),
        format_duration(output.duration),
        slide.command,
        output.stdout,
        output.stderr,
    )
}

fn format_duration(duration: Duration) -> String {
    if duration.as_secs() > 0 {
        format!("{:.2}s", duration.as_secs_f64())
    } else {
        format!("{}ms", duration.as_millis())
    }
}

fn footer_line(app: &App) -> Line<'static> {
    let slide = &app.commands[app.active.index];
    let mode = match app.mode {
        Mode::Presenting => "",
        Mode::ConfirmQuit => " | quit? y/n",
    };

    Line::from(vec![
        Span::styled(
            format!(" {}/{} ", app.active.index + 1, app.commands.len()),
            Style::default()
                .fg(Color::Black)
                .bg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(
            " {}:{} | h/l prev/next | j/k scroll | s shell | q quit{}",
            slide.file.display(),
            slide.line,
            mode,
        )),
    ])
}

fn render_quit_confirmation(frame: &mut Frame) {
    let area = centered_rect(48, 5, frame.area());
    let block = Block::default()
        .title(" Confirm quit ")
        .borders(Borders::ALL);
    let paragraph = Paragraph::new("Quit tuition?\n\nPress y/Enter to quit, n/Esc to cancel.")
        .block(block)
        .style(Style::default().fg(Color::Yellow));

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}

fn centered_rect(width: u16, height: u16, area: ratatui::layout::Rect) -> ratatui::layout::Rect {
    let popup_width = width.min(area.width);
    let popup_height = height.min(area.height);
    let x = area.x + area.width.saturating_sub(popup_width) / 2;
    let y = area.y + area.height.saturating_sub(popup_height) / 2;

    ratatui::layout::Rect {
        x,
        y,
        width: popup_width,
        height: popup_height,
    }
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
    fn command_runner_captures_successful_stdout() {
        let slide = SlideCommand {
            file: PathBuf::from("test"),
            line: 1,
            command: "printf hello".to_string(),
        };

        let output = run_slide_command(&slide);

        assert!(output.success);
        assert_eq!(output.stdout, "hello");
    }

    #[test]
    fn command_runner_captures_failure_diagnostics() {
        let slide = SlideCommand {
            file: PathBuf::from("test"),
            line: 1,
            command: "printf out; printf err >&2; exit 7".to_string(),
        };

        let output = run_slide_command(&slide);

        assert!(!output.success);
        assert_eq!(output.code, Some(7));
        assert_eq!(output.stdout, "out");
        assert_eq!(output.stderr, "err");
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
