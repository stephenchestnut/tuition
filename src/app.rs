use std::path::PathBuf;

use anyhow::{Result, anyhow};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{slides::SlideCommand, terminal::Terminal};

#[derive(Debug, Clone)]
pub struct ActiveSlide {
    pub index: usize,
}

#[derive(Debug)]
pub struct App {
    pub commands: Vec<SlideCommand>,
    pub active: ActiveSlide,
    pub aliases: Vec<PathBuf>,
}

impl App {
    pub fn new(commands: Vec<SlideCommand>, aliases: Vec<PathBuf>) -> Self {
        Self {
            commands,
            active: ActiveSlide { index: 0 },
            aliases,
        }
    }
}

pub trait CommandRunner {
    fn run_slide(
        &mut self,
        slide: &SlideCommand,
        status_bar: &str,
        aliases: &[PathBuf],
    ) -> Result<()>;

    fn open_temporary_shell(&mut self, aliases: &[PathBuf]) -> Result<()>;
}

pub fn run_app<T, R>(app: &mut App, terminal: &mut T, runner: &mut R) -> Result<()>
where
    T: Terminal,
    R: CommandRunner,
{
    render_active_slide(app, terminal, runner)?;

    loop {
        terminal.enable_raw_mode()?;
        let key = terminal.read_key();
        terminal.disable_raw_mode()?;
        let key = key?;

        if handle_key(app, key, terminal, runner)? {
            return Ok(());
        }
    }
}

pub fn run_single_slide<T, R>(
    app: &mut App,
    slide_number: usize,
    terminal: &mut T,
    runner: &mut R,
) -> Result<()>
where
    T: Terminal,
    R: CommandRunner,
{
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
    render_active_slide(app, terminal, runner)
}

fn handle_key<T, R>(app: &mut App, key: KeyEvent, terminal: &mut T, runner: &mut R) -> Result<bool>
where
    T: Terminal,
    R: CommandRunner,
{
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Ok(true);
    }

    match key.code {
        KeyCode::Char('l') | KeyCode::Right | KeyCode::Char(' ') => {
            next_slide(app, terminal, runner)?
        }
        KeyCode::Char('h') | KeyCode::Left => previous_slide(app, terminal, runner)?,
        KeyCode::Char('r') => render_active_slide(app, terminal, runner)?,
        KeyCode::Char('s') => open_temporary_shell(app, terminal, runner)?,
        KeyCode::Char('q') => return confirm_quit(terminal),
        _ => {}
    }

    Ok(false)
}

fn confirm_quit<T: Terminal>(terminal: &mut T) -> Result<bool> {
    terminal.print_status("\nQuit tuition? y/n ")?;

    terminal.enable_raw_mode()?;
    let key = terminal.read_key();
    terminal.disable_raw_mode()?;

    match key?.code {
        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter | KeyCode::Char('q') => Ok(true),
        _ => {
            terminal.print_status("cancelled\n")?;
            Ok(false)
        }
    }
}

fn next_slide<T, R>(app: &mut App, terminal: &mut T, runner: &mut R) -> Result<()>
where
    T: Terminal,
    R: CommandRunner,
{
    let next = app.active.index + 1;
    if next < app.commands.len() {
        activate_slide(app, next, terminal, runner)?;
    }
    Ok(())
}

fn previous_slide<T, R>(app: &mut App, terminal: &mut T, runner: &mut R) -> Result<()>
where
    T: Terminal,
    R: CommandRunner,
{
    if app.active.index > 0 {
        activate_slide(app, app.active.index - 1, terminal, runner)?;
    }
    Ok(())
}

fn activate_slide<T, R>(app: &mut App, index: usize, terminal: &mut T, runner: &mut R) -> Result<()>
where
    T: Terminal,
    R: CommandRunner,
{
    app.active.index = index;
    render_active_slide(app, terminal, runner)
}

fn render_active_slide<T, R>(app: &mut App, terminal: &mut T, runner: &mut R) -> Result<()>
where
    T: Terminal,
    R: CommandRunner,
{
    terminal.disable_raw_mode()?;
    let slide = &app.commands[app.active.index];
    let status_bar = status_bar(app);
    runner.run_slide(slide, &status_bar, &app.aliases)
}

fn open_temporary_shell<T, R>(app: &App, terminal: &mut T, runner: &mut R) -> Result<()>
where
    T: Terminal,
    R: CommandRunner,
{
    terminal.disable_raw_mode()?;
    runner.open_temporary_shell(&app.aliases)?;
    terminal.print_status(&format!("{}\n", status_bar(app)))
}

pub fn status_bar(app: &App) -> String {
    let slide = &app.commands[app.active.index];
    format!(
        "\x1b[7m {}/{} \x1b[0m {}:{} | (r)erun | (s)hell | (q)uit",
        app.active.index + 1,
        app.commands.len(),
        slide.file.display(),
        slide.line,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState};

    #[derive(Default)]
    struct FakeTerminal {
        keys: Vec<KeyEvent>,
        events: Vec<String>,
    }

    impl FakeTerminal {
        fn with_keys(keys: Vec<KeyEvent>) -> Self {
            let mut keys = keys;
            keys.reverse();
            Self {
                keys,
                events: Vec::new(),
            }
        }
    }

    impl Terminal for FakeTerminal {
        fn enable_raw_mode(&mut self) -> Result<()> {
            self.events.push("enable_raw".to_string());
            Ok(())
        }

        fn disable_raw_mode(&mut self) -> Result<()> {
            self.events.push("disable_raw".to_string());
            Ok(())
        }

        fn read_key(&mut self) -> Result<KeyEvent> {
            self.events.push("read_key".to_string());
            self.keys.pop().ok_or_else(|| anyhow!("no fake keys left"))
        }

        fn print_status(&mut self, status: &str) -> Result<()> {
            self.events.push(format!("print:{status}"));
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeCommandRunner {
        runs: Vec<SlideCommand>,
        statuses: Vec<String>,
        shells_opened: usize,
    }

    impl CommandRunner for FakeCommandRunner {
        fn run_slide(
            &mut self,
            slide: &SlideCommand,
            status_bar: &str,
            _aliases: &[PathBuf],
        ) -> Result<()> {
            self.runs.push(slide.clone());
            self.statuses.push(status_bar.to_string());
            Ok(())
        }

        fn open_temporary_shell(&mut self, _aliases: &[PathBuf]) -> Result<()> {
            self.shells_opened += 1;
            Ok(())
        }
    }

    #[test]
    fn run_single_slide_rejects_zero() {
        let mut app = test_app(vec![":"], vec![]);
        let mut terminal = FakeTerminal::default();
        let mut runner = FakeCommandRunner::default();
        let err = run_single_slide(&mut app, 0, &mut terminal, &mut runner)
            .unwrap_err()
            .to_string();

        assert!(err.contains("--slide must be at least 1"));
    }

    #[test]
    fn run_single_slide_rejects_out_of_range() {
        let mut app = test_app(vec![":"], vec![]);
        let mut terminal = FakeTerminal::default();
        let mut runner = FakeCommandRunner::default();
        let err = run_single_slide(&mut app, 2, &mut terminal, &mut runner)
            .unwrap_err()
            .to_string();

        assert!(err.contains("--slide 2 is out of range; there are 1 slides"));
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
    fn startup_runs_first_slide_once() {
        let mut app = test_app(vec!["one", "two"], vec![]);
        let mut terminal =
            FakeTerminal::with_keys(vec![key(KeyCode::Char('q')), key(KeyCode::Char('y'))]);
        let mut runner = FakeCommandRunner::default();

        run_app(&mut app, &mut terminal, &mut runner).unwrap();

        assert_eq!(
            runner
                .runs
                .iter()
                .map(|s| s.command.as_str())
                .collect::<Vec<_>>(),
            vec!["one"]
        );
    }

    #[test]
    fn next_key_advances_and_runs_next_slide() {
        let mut app = test_app(vec!["one", "two"], vec![]);
        let mut terminal = FakeTerminal::with_keys(vec![key(KeyCode::Char('l')), ctrl_c()]);
        let mut runner = FakeCommandRunner::default();

        run_app(&mut app, &mut terminal, &mut runner).unwrap();

        assert_eq!(
            runner
                .runs
                .iter()
                .map(|s| s.command.as_str())
                .collect::<Vec<_>>(),
            vec!["one", "two"]
        );
    }

    #[test]
    fn right_arrow_and_space_behave_like_l() {
        for code in [KeyCode::Right, KeyCode::Char(' ')] {
            let mut app = test_app(vec!["one", "two"], vec![]);
            let mut terminal = FakeTerminal::with_keys(vec![key(code), ctrl_c()]);
            let mut runner = FakeCommandRunner::default();

            run_app(&mut app, &mut terminal, &mut runner).unwrap();

            assert_eq!(
                runner
                    .runs
                    .iter()
                    .map(|s| s.command.as_str())
                    .collect::<Vec<_>>(),
                vec!["one", "two"]
            );
        }
    }

    #[test]
    fn previous_key_returns_and_runs_previous_slide() {
        let mut app = test_app(vec!["one", "two"], vec![]);
        app.active.index = 1;
        let mut terminal = FakeTerminal::with_keys(vec![key(KeyCode::Char('h')), ctrl_c()]);
        let mut runner = FakeCommandRunner::default();

        run_app(&mut app, &mut terminal, &mut runner).unwrap();

        assert_eq!(
            runner
                .runs
                .iter()
                .map(|s| s.command.as_str())
                .collect::<Vec<_>>(),
            vec!["two", "one"]
        );
    }

    #[test]
    fn attempting_to_move_beyond_bounds_does_not_rerun() {
        let mut app = test_app(vec!["one"], vec![]);
        let mut terminal = FakeTerminal::with_keys(vec![
            key(KeyCode::Char('h')),
            key(KeyCode::Char('l')),
            ctrl_c(),
        ]);
        let mut runner = FakeCommandRunner::default();

        run_app(&mut app, &mut terminal, &mut runner).unwrap();

        assert_eq!(runner.runs.len(), 1);
    }

    #[test]
    fn r_reruns_current_slide() {
        let mut app = test_app(vec!["one"], vec![]);
        let mut terminal = FakeTerminal::with_keys(vec![key(KeyCode::Char('r')), ctrl_c()]);
        let mut runner = FakeCommandRunner::default();

        run_app(&mut app, &mut terminal, &mut runner).unwrap();

        assert_eq!(
            runner
                .runs
                .iter()
                .map(|s| s.command.as_str())
                .collect::<Vec<_>>(),
            vec!["one", "one"]
        );
    }

    #[test]
    fn s_opens_shell_without_rerunning_current_slide() {
        let mut app = test_app(vec!["one"], vec![]);
        let mut terminal = FakeTerminal::with_keys(vec![key(KeyCode::Char('s')), ctrl_c()]);
        let mut runner = FakeCommandRunner::default();

        run_app(&mut app, &mut terminal, &mut runner).unwrap();

        assert_eq!(runner.runs.len(), 1);
        assert_eq!(runner.shells_opened, 1);
        assert!(
            terminal
                .events
                .iter()
                .any(|event| event.starts_with("print:\x1b[7m 1/1"))
        );
    }

    #[test]
    fn cancelled_quit_continues_app() {
        let mut app = test_app(vec!["one", "two"], vec![]);
        let mut terminal = FakeTerminal::with_keys(vec![
            key(KeyCode::Char('q')),
            key(KeyCode::Char('n')),
            key(KeyCode::Char('l')),
            ctrl_c(),
        ]);
        let mut runner = FakeCommandRunner::default();

        run_app(&mut app, &mut terminal, &mut runner).unwrap();

        assert_eq!(
            runner
                .runs
                .iter()
                .map(|s| s.command.as_str())
                .collect::<Vec<_>>(),
            vec!["one", "two"]
        );
    }

    #[test]
    fn confirmed_quit_exits() {
        let mut app = test_app(vec!["one"], vec![]);
        let mut terminal =
            FakeTerminal::with_keys(vec![key(KeyCode::Char('q')), key(KeyCode::Enter)]);
        let mut runner = FakeCommandRunner::default();

        run_app(&mut app, &mut terminal, &mut runner).unwrap();

        assert_eq!(runner.runs.len(), 1);
    }

    #[test]
    fn ctrl_c_exits_immediately() {
        let mut app = test_app(vec!["one", "two"], vec![]);
        let mut terminal = FakeTerminal::with_keys(vec![ctrl_c(), key(KeyCode::Char('l'))]);
        let mut runner = FakeCommandRunner::default();

        run_app(&mut app, &mut terminal, &mut runner).unwrap();

        assert_eq!(runner.runs.len(), 1);
    }

    #[test]
    fn unknown_keys_do_nothing() {
        let mut app = test_app(vec!["one", "two"], vec![]);
        let mut terminal = FakeTerminal::with_keys(vec![key(KeyCode::Char('x')), ctrl_c()]);
        let mut runner = FakeCommandRunner::default();

        run_app(&mut app, &mut terminal, &mut runner).unwrap();

        assert_eq!(runner.runs.len(), 1);
    }

    #[test]
    fn raw_mode_is_disabled_around_command_execution() {
        let mut app = test_app(vec!["one", "two"], vec![]);
        let mut terminal = FakeTerminal::with_keys(vec![key(KeyCode::Char('l')), ctrl_c()]);
        let mut runner = FakeCommandRunner::default();

        run_app(&mut app, &mut terminal, &mut runner).unwrap();

        let expected_prefix = [
            "disable_raw",
            "enable_raw",
            "read_key",
            "disable_raw",
            "disable_raw",
        ];
        assert_eq!(terminal.events[..expected_prefix.len()], expected_prefix);
    }

    fn test_app(commands: Vec<&str>, aliases: Vec<PathBuf>) -> App {
        App::new(
            commands
                .into_iter()
                .enumerate()
                .map(|(idx, command)| SlideCommand {
                    file: PathBuf::from("slides.txt"),
                    line: idx + 1,
                    command: command.to_string(),
                })
                .collect(),
            aliases,
        )
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::empty(),
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        }
    }

    fn ctrl_c() -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char('c'),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        }
    }
}
