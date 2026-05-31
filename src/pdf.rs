use std::{
    env,
    fs::File,
    io::{BufWriter, Read},
    path::{Path, PathBuf},
    thread,
};

use anyhow::{Context, Result, anyhow};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use printpdf::{
    BuiltinFont, Color as PdfColor, Image, ImageTransform, Mm, PdfDocument, Rgb as PdfRgb,
    image_crate::{DynamicImage, GenericImageView, RgbImage},
};
use vt100::Color as VtColor;

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
const PX_TO_MM: f64 = 25.4 / 96.0;
const MAX_IMAGE_WIDTH_MM: f64 = 90.0;

#[derive(Debug)]
struct RenderedSlide {
    rows: Vec<Vec<RenderedCell>>,
    images: Vec<RenderedImage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RenderedCell {
    text: String,
    fg: VtColor,
    bg: VtColor,
    bold: bool,
    inverse: bool,
}

#[derive(Debug)]
struct RenderedImage {
    row: u16,
    col: u16,
    bytes: Vec<u8>,
    requested_width_px: Option<u32>,
}

#[derive(Debug)]
struct LayoutImage<'a> {
    image: &'a RenderedImage,
    width_mm: f64,
    height_mm: f64,
    extra_space_mm: f64,
    prior_extra_space_mm: f64,
}

#[derive(Debug)]
struct PendingImage {
    row: u16,
    col: u16,
    data: String,
    requested_width_px: Option<u32>,
}

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
) -> Result<RenderedSlide> {
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

    let mut rendered = process_terminal_output(&bytes, cols, rows);
    if rendered.images.is_empty() {
        rendered
            .images
            .extend(fallback_imgcat_images(slide, &rendered.rows));
    }
    Ok(rendered)
}

fn process_terminal_output(bytes: &[u8], cols: u16, rows: u16) -> RenderedSlide {
    let mut parser = vt100::Parser::new(rows, cols, 0);
    let mut images = Vec::new();
    let mut pending_image: Option<PendingImage> = None;
    let mut start = 0;
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == 0x1b && bytes.get(i + 1) == Some(&b']') {
            parser.process(&bytes[start..i]);
            if let Some((end, osc)) = read_osc(&bytes[i + 2..]) {
                handle_osc(&osc, &parser, &mut pending_image, &mut images);
                i += end + 2;
                start = i;
                continue;
            }
        }
        i += 1;
    }
    parser.process(&bytes[start..]);

    RenderedSlide {
        rows: rendered_cells(&parser, cols, rows),
        images,
    }
}

fn read_osc(bytes: &[u8]) -> Option<(usize, String)> {
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x07 {
            return Some((i + 1, String::from_utf8_lossy(&bytes[..i]).into_owned()));
        }
        if bytes[i] == 0x1b && bytes.get(i + 1) == Some(&b'\\') {
            return Some((i + 2, String::from_utf8_lossy(&bytes[..i]).into_owned()));
        }
        i += 1;
    }
    None
}

fn handle_osc(
    osc: &str,
    parser: &vt100::Parser,
    pending_image: &mut Option<PendingImage>,
    images: &mut Vec<RenderedImage>,
) {
    let Some(rest) = osc.strip_prefix("1337;") else {
        return;
    };

    if let Some(metadata) = rest.strip_prefix("MultipartFile=") {
        let (row, col) = parser.screen().cursor_position();
        *pending_image = Some(PendingImage {
            row,
            col,
            data: String::new(),
            requested_width_px: parse_width_px(metadata),
        });
    } else if let Some(chunk) = rest.strip_prefix("FilePart=") {
        if let Some(image) = pending_image.as_mut() {
            image.data.push_str(chunk);
        }
    } else if rest == "FileEnd" {
        if let Some(image) = pending_image.take()
            && let Ok(bytes) = BASE64.decode(image.data.as_bytes())
        {
            images.push(RenderedImage {
                row: image.row,
                col: image.col,
                bytes,
                requested_width_px: image.requested_width_px,
            });
        }
    } else if let Some(file) = rest.strip_prefix("File=") {
        let Some((metadata, data)) = file.split_once(':') else {
            return;
        };
        if let Ok(bytes) = BASE64.decode(data.as_bytes()) {
            let (row, col) = parser.screen().cursor_position();
            images.push(RenderedImage {
                row,
                col,
                bytes,
                requested_width_px: parse_width_px(metadata),
            });
        }
    }
}

fn parse_width_px(metadata: &str) -> Option<u32> {
    metadata
        .split(';')
        .find_map(|part| part.strip_prefix("width="))
        .and_then(|width| width.strip_suffix("px"))
        .and_then(|width| width.parse().ok())
}

fn rendered_cells(parser: &vt100::Parser, cols: u16, rows: u16) -> Vec<Vec<RenderedCell>> {
    (0..rows)
        .map(|row| {
            let mut cells = Vec::new();
            for col in 0..cols {
                let Some(cell) = parser.screen().cell(row, col) else {
                    continue;
                };
                if cell.is_wide_continuation() {
                    continue;
                }
                cells.push(RenderedCell {
                    text: if cell.has_contents() {
                        cell.contents()
                    } else {
                        " ".to_string()
                    },
                    fg: cell.fgcolor(),
                    bg: cell.bgcolor(),
                    bold: cell.bold(),
                    inverse: cell.inverse(),
                });
            }
            while matches!(cells.last(), Some(cell) if cell.text == " " && cell.bg == VtColor::Default) {
                cells.pop();
            }
            cells
        })
        .collect()
}

fn fallback_imgcat_images(slide: &SlideCommand, rows: &[Vec<RenderedCell>]) -> Vec<RenderedImage> {
    let tokens = shell_like_tokens(&slide.command);
    let Some(imgcat_pos) = tokens
        .iter()
        .position(|token| token == "imgcat" || token.rsplit('/').next() == Some("imgcat"))
    else {
        return Vec::new();
    };

    let mut requested_width_px = None;
    let mut path = None;
    let mut idx = imgcat_pos + 1;
    while idx < tokens.len() {
        let token = &tokens[idx];
        if token == ";" || token == "&&" || token == "||" || token == "printf" {
            break;
        }
        if token == "-W" || token == "--width" {
            requested_width_px = tokens
                .get(idx + 1)
                .and_then(|width| parse_width_token(width));
            idx += 2;
            continue;
        }
        if let Some(width) = token.strip_prefix("-W") {
            requested_width_px = parse_width_token(width);
            idx += 1;
            continue;
        }
        if token.starts_with('-') {
            idx += 1;
            continue;
        }
        path = Some(token.clone());
        break;
    }

    let Some(path) = path else {
        return Vec::new();
    };
    let path = slide_command_cwd(slide).join(path);
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };

    vec![RenderedImage {
        row: fallback_image_row(rows),
        col: 0,
        bytes,
        requested_width_px,
    }]
}

fn fallback_image_row(rows: &[Vec<RenderedCell>]) -> u16 {
    rows.iter()
        .enumerate()
        .skip(1)
        .find(|(_, row)| row.is_empty())
        .map(|(idx, _)| idx as u16)
        .unwrap_or(1)
}

fn parse_width_token(width: &str) -> Option<u32> {
    width.strip_suffix("px").unwrap_or(width).parse().ok()
}

fn shell_like_tokens(command: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut chars = command.chars().peekable();
    let mut quote = None;

    while let Some(ch) = chars.next() {
        if let Some(quote_ch) = quote {
            if ch == quote_ch {
                quote = None;
            } else {
                token.push(ch);
            }
            continue;
        }

        match ch {
            '\'' | '"' => quote = Some(ch),
            ';' => {
                if !token.is_empty() {
                    tokens.push(std::mem::take(&mut token));
                }
                tokens.push(";".to_string());
            }
            '&' if chars.peek() == Some(&'&') => {
                chars.next();
                if !token.is_empty() {
                    tokens.push(std::mem::take(&mut token));
                }
                tokens.push("&&".to_string());
            }
            '|' if chars.peek() == Some(&'|') => {
                chars.next();
                if !token.is_empty() {
                    tokens.push(std::mem::take(&mut token));
                }
                tokens.push("||".to_string());
            }
            ch if ch.is_whitespace() => {
                if !token.is_empty() {
                    tokens.push(std::mem::take(&mut token));
                }
            }
            _ => token.push(ch),
        }
    }
    if !token.is_empty() {
        tokens.push(token);
    }
    tokens
}

fn write_pdf(slides: &[RenderedSlide], output: &Path, cols: u16, rows: u16) -> Result<()> {
    let (doc, first_page, first_layer) = PdfDocument::new(
        "tuition export",
        Mm(PAGE_WIDTH_MM as f32),
        Mm(PAGE_HEIGHT_MM as f32),
        "Slide 1",
    );
    let font = doc
        .add_builtin_font(BuiltinFont::Courier)
        .context("failed to load built-in PDF font")?;

    for (idx, slide) in slides.iter().enumerate() {
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
        let cell_width_mm = font_size * 0.6 * PT_TO_MM;
        let line_height_mm = font_size * 1.2 * PT_TO_MM;
        let start_y = PAGE_HEIGHT_MM - MARGIN_MM - font_size * PT_TO_MM;
        let layout_images = layout_images(&slide.images, line_height_mm);

        for image in &layout_images {
            add_image_to_layer(&layer, image, cell_width_mm, line_height_mm)?;
        }

        for (row, cells) in slide.rows.iter().enumerate() {
            let y = start_y
                - row as f64 * line_height_mm
                - extra_image_space_before_row(&layout_images, row as u16);
            if y < MARGIN_MM / 2.0 {
                break;
            }
            render_text_row(&layer, cells, y, font_size, cell_width_mm, &font);
        }
    }

    let file = File::create(output)
        .with_context(|| format!("failed to create PDF output {}", output.display()))?;
    doc.save(&mut BufWriter::new(file))
        .with_context(|| format!("failed to write PDF output {}", output.display()))
}

fn render_text_row(
    layer: &printpdf::PdfLayerReference,
    cells: &[RenderedCell],
    y: f64,
    font_size: f64,
    cell_width_mm: f64,
    font: &printpdf::IndirectFontRef,
) {
    let mut col = 0;
    while col < cells.len() {
        let first = &cells[col];
        if first.text == " " && first.bg == VtColor::Default {
            col += 1;
            continue;
        }

        let fg = effective_fg(first);
        let mut text = String::new();
        let start_col = col;
        while col < cells.len() && effective_fg(&cells[col]) == fg {
            text.push_str(&cells[col].text);
            col += 1;
        }

        if text.trim_end().is_empty() {
            continue;
        }
        layer.set_fill_color(pdf_color(fg));
        layer.use_text(
            text.trim_end(),
            font_size as f32,
            Mm((MARGIN_MM + start_col as f64 * cell_width_mm) as f32),
            Mm(y as f32),
            font,
        );
    }
}

fn layout_images(images: &[RenderedImage], line_height_mm: f64) -> Vec<LayoutImage<'_>> {
    let mut layout = Vec::new();
    let mut prior_extra_space_mm = 0.0;

    for image in images {
        let Some((width_mm, height_mm)) = image_pdf_size(image) else {
            continue;
        };
        let extra_space_mm = (height_mm - line_height_mm).max(0.0);
        layout.push(LayoutImage {
            image,
            width_mm,
            height_mm,
            extra_space_mm,
            prior_extra_space_mm,
        });
        prior_extra_space_mm += extra_space_mm;
    }

    layout
}

fn extra_image_space_before_row(images: &[LayoutImage<'_>], row: u16) -> f64 {
    images
        .iter()
        .filter(|image| image.image.row < row)
        .map(|image| image.extra_space_mm)
        .sum()
}

fn image_pdf_size(rendered: &RenderedImage) -> Option<(f64, f64)> {
    let image = printpdf::image_crate::load_from_memory(&rendered.bytes).ok()?;
    let (width_px, height_px) = image.dimensions();
    let default_width_mm = f64::from(width_px) / 300.0 * 25.4;
    let requested_width_mm = rendered
        .requested_width_px
        .map(|width| f64::from(width) * PX_TO_MM)
        .unwrap_or(default_width_mm);
    let available_width_mm = PAGE_WIDTH_MM - MARGIN_MM * 2.0;
    let width_mm = requested_width_mm
        .min(MAX_IMAGE_WIDTH_MM)
        .min(available_width_mm)
        .max(1.0);
    let height_mm = width_mm * f64::from(height_px) / f64::from(width_px);
    Some((width_mm, height_mm))
}

fn add_image_to_layer(
    layer: &printpdf::PdfLayerReference,
    layout: &LayoutImage<'_>,
    cell_width_mm: f64,
    line_height_mm: f64,
) -> Result<()> {
    let rendered = layout.image;
    let image = printpdf::image_crate::load_from_memory(&rendered.bytes)
        .context("failed to decode terminal image for PDF export")?;
    let (width_px, height_px) = image.dimensions();
    let image = flatten_alpha_on_white(image);
    let default_width_mm = f64::from(width_px) / 300.0 * 25.4;
    let default_height_mm = f64::from(height_px) / 300.0 * 25.4;
    let top_y = PAGE_HEIGHT_MM
        - MARGIN_MM
        - f64::from(rendered.row) * line_height_mm
        - layout.prior_extra_space_mm;
    let bottom_y = (top_y - layout.height_mm).max(MARGIN_MM);

    Image::from_dynamic_image(&image).add_to_layer(
        layer.clone(),
        ImageTransform {
            translate_x: Some(Mm(
                (MARGIN_MM + f64::from(rendered.col) * cell_width_mm) as f32
            )),
            translate_y: Some(Mm(bottom_y as f32)),
            scale_x: Some((layout.width_mm / default_width_mm) as f32),
            scale_y: Some((layout.height_mm / default_height_mm) as f32),
            dpi: Some(300.0),
            ..ImageTransform::default()
        },
    );
    Ok(())
}

fn flatten_alpha_on_white(image: DynamicImage) -> DynamicImage {
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    let mut rgb = RgbImage::new(width, height);

    for (x, y, pixel) in rgba.enumerate_pixels() {
        let [r, g, b, a] = pixel.0;
        let alpha = u16::from(a);
        let blend = |channel: u8| -> u8 {
            ((u16::from(channel) * alpha + 255 * (255 - alpha)) / 255) as u8
        };
        rgb.put_pixel(
            x,
            y,
            printpdf::image_crate::Rgb([blend(r), blend(g), blend(b)]),
        );
    }

    DynamicImage::ImageRgb8(rgb)
}

fn effective_fg(cell: &RenderedCell) -> VtColor {
    if cell.inverse {
        if cell.bg == VtColor::Default {
            VtColor::Idx(0)
        } else {
            cell.bg
        }
    } else if cell.bold {
        match cell.fg {
            VtColor::Idx(idx @ 0..=7) => VtColor::Idx(idx + 8),
            color => color,
        }
    } else {
        cell.fg
    }
}

fn pdf_color(color: VtColor) -> PdfColor {
    let (r, g, b) = match color {
        VtColor::Default => (0, 0, 0),
        VtColor::Rgb(r, g, b) => (r, g, b),
        VtColor::Idx(idx) => ansi_color(idx),
    };
    PdfColor::Rgb(PdfRgb::new(
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
        None,
    ))
}

fn ansi_color(idx: u8) -> (u8, u8, u8) {
    const ANSI_16: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (205, 49, 49),
        (13, 188, 121),
        (229, 229, 16),
        (36, 114, 200),
        (188, 63, 188),
        (17, 168, 205),
        (229, 229, 229),
        (102, 102, 102),
        (241, 76, 76),
        (35, 209, 139),
        (245, 245, 67),
        (59, 142, 234),
        (214, 112, 214),
        (41, 184, 219),
        (255, 255, 255),
    ];
    if let Some(color) = ANSI_16.get(usize::from(idx)) {
        return *color;
    }
    if idx >= 232 {
        let level = 8 + (idx - 232) * 10;
        return (level, level, level);
    }
    let idx = idx.saturating_sub(16);
    let component = |n: u8| if n == 0 { 0 } else { 55 + n * 40 };
    (
        component(idx / 36),
        component((idx / 6) % 6),
        component(idx % 6),
    )
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

    #[test]
    fn ansi_bold_color_uses_bright_variant() {
        let cell = RenderedCell {
            text: "x".to_string(),
            fg: VtColor::Idx(6),
            bg: VtColor::Default,
            bold: true,
            inverse: false,
        };

        assert_eq!(effective_fg(&cell), VtColor::Idx(14));
    }

    #[test]
    fn parses_iterm2_file_width() {
        assert_eq!(parse_width_px("inline=1;width=500px;size=10"), Some(500));
    }

    #[test]
    fn extracts_iterm2_multipart_image() {
        let output = b"before\n\x1b]1337;MultipartFile=inline=1;width=10px\x07\x1b]1337;FilePart=aGk=\x07\x1b]1337;FileEnd\x07after\n";
        let slide = process_terminal_output(output, 80, 24);

        assert_eq!(slide.images.len(), 1);
        assert_eq!(slide.images[0].bytes, b"hi");
        assert_eq!(slide.images[0].requested_width_px, Some(10));
    }

    #[test]
    fn tokenizes_imgcat_command_for_fallback() {
        assert_eq!(
            shell_like_tokens("printf 'x y'; imgcat -W 500px -s ../logo.png; printf done"),
            vec![
                "printf",
                "x y",
                ";",
                "imgcat",
                "-W",
                "500px",
                "-s",
                "../logo.png",
                ";",
                "printf",
                "done",
            ]
        );
    }
}
