use std::io::{self, Write};

use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyEvent},
    terminal::{disable_raw_mode, enable_raw_mode},
};

pub trait Terminal {
    fn enable_raw_mode(&mut self) -> Result<()>;
    fn disable_raw_mode(&mut self) -> Result<()>;
    fn read_key(&mut self) -> Result<KeyEvent>;
    fn print_status(&mut self, status: &str) -> Result<()>;
}

pub struct CrosstermTerminal;

impl Terminal for CrosstermTerminal {
    fn enable_raw_mode(&mut self) -> Result<()> {
        enable_raw_mode().context("failed to enable raw mode")
    }

    fn disable_raw_mode(&mut self) -> Result<()> {
        disable_raw_mode().context("failed to disable raw mode")
    }

    fn read_key(&mut self) -> Result<KeyEvent> {
        loop {
            if let Event::Key(key) = event::read().context("failed to read terminal event")? {
                return Ok(key);
            }
        }
    }

    fn print_status(&mut self, status: &str) -> Result<()> {
        print!("{status}");
        io::stdout().flush().context("failed to flush stdout")
    }
}

pub fn cleanup_terminal() -> Result<()> {
    let _ = disable_raw_mode();
    Ok(())
}
