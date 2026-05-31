use std::{
    env,
    fs::File,
    io::{BufWriter, Read},
    path::{Path, PathBuf},
    thread,
};

use anyhow::{Context, Result, anyhow};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use printpdf::{BuiltinFont, Mm, PdfDocument};

use crate::{
    shell::{shell_name, slide_pdf_script},
    slides::{SlideCommand, slide_command_cwd},
};

const DEFAULT_COLS: u16 = 100;
const DEFAULT_ROWS: u16 = 30;
const PAGE_WIDTH_MM: f64 = 279.4; // US Letter landscape
const PAGE_HEIGHT_MM: f64 = 215.9;
const MARGIN_MM: f64 = 12.0;
const PT_TO_MM: f64 = 0.352_777_778;

pub fn export_pdf(
    slides: &[SlideCommand],
    aliases: &[PathBuf],
    output: &Path,
    requested_cols: u16,
    requested_rows: u16,
) -> Result<()> {
    let (cols, rows) = pdf_terminal_size(requested_cols, requested_rows);
    if cols == 0 || rows == 0 {
        return Err(anyhow!("PDF terminal dimensions must be non-zero"));
    }

    let mut rendered = Vec::with_capacity(slides.len());
    for slide in slides {
        rendered.push(render_slide(slide, aliases, cols, rows)?);
    }

    write_pdf(&rendered, output, cols, rows)
}

pub fn pdf_terminal_size(requested_cols: u16, requested_rows: u16) -> (u16, u16) {
    let detected = if requested_cols == 0 || requested_rows == 0 {
        crossterm::terminal::size().ok()
    } else {
        None
    };
    let (detected_cols, detected_rows) = detected.unwrap_or((DEFAULT_COLS, DEFAULT_ROWS));

    (
        if requested_cols == 0 {
            detected_cols
        } else {
            requested_cols
        },
        if requested_rows == 0 {
            detected_rows
        } else {
            requested_rows
        },
    )
}

fn render_slide(
    slide: &SlideCommand,
    aliases: &[PathBuf],
    cols: u16,
    rows: u16,
) -> Result<Vec<String>> {
    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let shell_name = shell_name(&shell);
    let script = slide_pdf_script(slide, shell_name.as_deref(), aliases);

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .context("failed to open PTY for PDF export")?;

    let mut command = CommandBuilder::new(shell);
    command.arg("-lc");
    command.arg(script);
    command.cwd(slide_command_cwd(slide));

    let mut child = pair
        .slave
        .spawn_command(command)
        .with_context(|| format!("failed to run slide {} for PDF export", slide.line))?;
    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .context("failed to read from PDF export PTY")?;
    let reader_thread = thread::spawn(move || -> std::io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        Ok(bytes)
    });

    child
        .wait()
        .with_context(|| format!("failed waiting for slide {} during PDF export", slide.line))?;
    drop(pair.master);

    let bytes = reader_thread
        .join()
        .map_err(|_| anyhow!("PDF export PTY reader thread panicked"))?
        .context("failed reading PDF export PTY output")?;

    let mut parser = vt100::Parser::new(rows, cols, 0);
    parser.process(&bytes);

    Ok(parser
        .screen()
        .rows(0, cols)
        .map(|line| line.trim_end().to_string())
        .collect())
}

fn write_pdf(slides: &[Vec<String>], output: &Path, cols: u16, rows: u16) -> Result<()> {
    let (doc, first_page, first_layer) = PdfDocument::new(
        "tuition export",
        Mm(PAGE_WIDTH_MM as f32),
        Mm(PAGE_HEIGHT_MM as f32),
        "Slide 1",
    );
    let font = doc
        .add_builtin_font(BuiltinFont::Courier)
        .context("failed to load built-in PDF font")?;

    for (idx, lines) in slides.iter().enumerate() {
        let (page, layer) = if idx == 0 {
            (first_page, first_layer)
        } else {
            doc.add_page(
                Mm(PAGE_WIDTH_MM as f32),
                Mm(PAGE_HEIGHT_MM as f32),
                format!("Slide {}", idx + 1),
            )
        };
        let layer = doc.get_page(page).get_layer(layer);
        let font_size = font_size_for_grid(cols, rows);
        let line_height_mm = font_size * 1.2 * PT_TO_MM;
        let start_y = PAGE_HEIGHT_MM - MARGIN_MM - font_size * PT_TO_MM;

        for (row, line) in lines.iter().enumerate() {
            if line.is_empty() {
                continue;
            }
            let y = start_y - row as f64 * line_height_mm;
            if y < MARGIN_MM / 2.0 {
                break;
            }
            layer.use_text(
                line,
                font_size as f32,
                Mm(MARGIN_MM as f32),
                Mm(y as f32),
                &font,
            );
        }
    }

    let file = File::create(output)
        .with_context(|| format!("failed to create PDF output {}", output.display()))?;
    doc.save(&mut BufWriter::new(file))
        .with_context(|| format!("failed to write PDF output {}", output.display()))
}

fn font_size_for_grid(cols: u16, rows: u16) -> f64 {
    let available_width_mm = PAGE_WIDTH_MM - MARGIN_MM * 2.0;
    let available_height_mm = PAGE_HEIGHT_MM - MARGIN_MM * 2.0;
    let by_width = available_width_mm / (f64::from(cols) * 0.6 * PT_TO_MM);
    let by_height = available_height_mm / (f64::from(rows) * 1.2 * PT_TO_MM);
    by_width.min(by_height).clamp(3.0, 14.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_size_uses_supplied_dimensions() {
        assert_eq!(pdf_terminal_size(80, 24), (80, 24));
    }

    #[test]
    fn terminal_size_fills_missing_dimension() {
        assert_eq!(pdf_terminal_size(120, 0).0, 120);
        assert_eq!(pdf_terminal_size(0, 40).1, 40);
    }

    #[test]
    fn font_size_is_positive() {
        assert!(font_size_for_grid(100, 30) > 0.0);
    }
}
