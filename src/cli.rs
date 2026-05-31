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

    /// Export the presentation to a PDF file instead of presenting interactively.
    #[arg(long, value_name = "FILE", conflicts_with = "slide")]
    pub pdf: Option<PathBuf>,

    /// Terminal columns to use for PDF export.
    #[arg(long, value_name = "COLS", default_value_t = 0)]
    pub pdfcols: u16,

    /// Terminal rows to use for PDF export.
    #[arg(long, value_name = "ROWS", default_value_t = 0)]
    pub pdfrows: u16,

    /// Query the calling terminal and use its default text/background colors for PDF export.
    #[arg(long, requires = "pdf")]
    pub capture_terminal_style: bool,

    /// Files containing slide commands, with optional backslash line continuations.
    #[arg(required = true)]
    pub files: Vec<PathBuf>,
}
