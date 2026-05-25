use anyhow::{Result, anyhow};

pub mod aliases;
pub mod app;
pub mod cli;
pub mod shell;
pub mod slides;
pub mod terminal;

use aliases::parse_aliases_arg;
use app::{App, run_app, run_single_slide};
use cli::Cli;
use shell::ShellCommandRunner;
use slides::parse_slide_files;
use terminal::{CrosstermTerminal, cleanup_terminal};

pub fn run(cli: Cli) -> Result<()> {
    let commands = parse_slide_files(&cli.files)?;
    if commands.is_empty() {
        return Err(anyhow!("no slide commands found"));
    }

    let mut app = App::new(commands, parse_aliases_arg(cli.aliases.as_deref())?);
    let mut terminal = CrosstermTerminal;
    let mut runner = ShellCommandRunner;

    let result = if let Some(slide_number) = cli.slide {
        run_single_slide(&mut app, slide_number, &mut terminal, &mut runner)
    } else {
        run_app(&mut app, &mut terminal, &mut runner)
    };
    let cleanup_result = cleanup_terminal();

    result.and(cleanup_result)
}
