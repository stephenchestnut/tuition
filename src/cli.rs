use std::path::PathBuf;

use clap::Parser;

#[derive(Debug, Parser)]
#[command(author, version, about)]
pub struct Cli {
    /// Source these aliases files before running slides and temporary shells.
    /// Separate multiple files with semicolons, for example: --aliases './aliases;./more-aliases'.
    #[arg(long)]
    pub aliases: Option<String>,

    /// Print and run just one slide, then exit.
    #[arg(long)]
    pub slide: Option<usize>,

    /// Files containing slide commands, one command per non-blank non-comment line.
    #[arg(required = true)]
    pub files: Vec<PathBuf>,
}
