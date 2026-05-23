use anyhow::Result;
use clap::Parser;
use tuition::cli::Cli;

fn main() -> Result<()> {
    tuition::run(Cli::parse())
}
