//! Conversions that go beyond pictures and media: PDF pages to images, documents to and from
//! PDF/Word/HTML/text/Markdown, and SVG to pictures.
//!
//! Documents are turned into a small neutral list of blocks (headings, paragraphs, tables) and written back
//! out in the wanted format. PDF pages are drawn with PDFium (the engine Chrome uses). Turning HTML into a PDF
//! uses Microsoft Edge, which every Windows 11 PC already has.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use image::DynamicImage;
use pdfium_render::prelude::*;
use quick_xml::events::Event;
use quick_xml::Reader;

use crate::process::{self, Done, Req};

const NO_WINDOW: u32 = 0x0800_0000;
/// Pixel width pages are drawn at (about 200 dpi for an A4 page).
const PAGE_WIDTH: i32 = 1654;
const IMAGE_TARGETS: [&str; 8] = ["png", "jpg", "webp", "bmp", "tiff", "gif", "ico", "avif"];

type Res<T> = Result<T, String>;

fn err<E: ToString>(e: E) -> String {
    e.to_string()
}

pub fn convert(path: &Path, req: &Req, rep: &dyn Fn(f64, u64)) -> Res<Done> {
    let target = req.option.as_str();
    let src = process::ext_lower(path);
    if src == target || (src == "htm" && target == "html") {
        return Err(format!("It's already a {}.", target.to_uppercase()));
    }
    match process::kind_of(path) {
        process::Kind::Svg => svg_convert(path, target, rep),
        process::Kind::Pdf => pdf_convert(path, target, rep),
        process::Kind::Doc => doc_convert(path, &src, target, rep),
        _ => Err("This kind of file can't be converted here.".into()),
    }
}

// ------------------------------------------------------------------ blocks

#[derive(Clone)]
struct Run {
    text: String,
    bold: bool,
    italic: bool,
}

#[derive(Clone)]
enum Block {
    /// level 0 is a normal paragraph, 1 to 3 are headings
    Para { level: u8, runs: Vec<Run> },
    Table(Vec<Vec<String>>),
}

fn plain(text: &str) -> Run {
    Run { text: text.to_string(), bold: false, italic: false }
}

fn para(text: &str) -> Block {
    Block::Para { level: 0, runs: vec![plain(text)] }
}

fn esc_html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn run_text(runs: &[Run]) -> String {
    runs.iter().map(|r| r.text.as_str()).collect()
}

fn blocks_to_html(blocks: &[Block], title: &str) -> String {
    let mut body = String::new();
    for b in blocks {
        match b {
            Block::Para { level, runs } => {
                let inner: String = runs
                    .iter()
                    .map(|r| {
                        let mut t = esc_html(&r.text).replace('\n', "<br>").replace('\t', "&emsp;");
                        if r.bold {
                            t = format!("<b>{t}</b>");
                        }
                        if r.italic {
                            t = format!("<i>{t}</i>");
                        }
                        t
                    })
                    .collect();
                if *level == 0 {
                    body += &format!("<p>{}</p>\n", if inner.trim().is_empty() { "&nbsp;".to_string() } else { inner });
                } else {
                    body += &format!("<h{level}>{inner}</h{level}>\n");
                }
            }
            Block::Table(rows) => {
                body += "<table>\n";
                for (i, row) in rows.iter().enumerate() {
                    let tag = if i == 0 { "th" } else { "td" };
                    body += "<tr>";
                    for cell in row {
                        body += &format!("<{tag}>{}</{tag}>", esc_html(cell));
                    }
                    body += "</tr>\n";
                }
                body += "</table>\n";
            }
        }
    }
    wrap_html(&body, title)
}

fn wrap_html(body: &str, title: &str) -> String {
    format!(
        "<!doctype html>\n<html><head><meta charset=\"utf-8\"><title>{}</title>\n<style>\n@page {{ size: A4; margin: 18mm; }}\nbody {{ font-family: 'Segoe UI', Calibri, Arial, sans-serif; font-size: 11pt; line-height: 1.5; color: #111; }}\nh1 {{ font-size: 22pt; }} h2 {{ font-size: 17pt; }} h3 {{ font-size: 13pt; }}\np {{ margin: 0 0 8pt; }}\ntable {{ border-collapse: collapse; margin: 8pt 0; }}\nth, td {{ border: 1px solid #999; padding: 4pt 8pt; text-align: left; }}\nth {{ background: #eef2fb; }}\npre {{ white-space: pre-wrap; }}\n</style></head><body>\n{body}</body></html>\n",
        esc_html(title)
    )
}

fn blocks_to_text(blocks: &[Block]) -> String {
    let mut out = String::new();
    for b in blocks {
        match b {
            Block::Para { runs, .. } => {
                out += &run_text(runs);
                out += "\n";
            }
            Block::Table(rows) => {
                for row in rows {
                    out += &row.join("\t");
                    out += "\n";
                }
            }
        }
    }
    out
}

fn blocks_to_md(blocks: &[Block]) -> String {
    let mut out = String::new();
    for b in blocks {
        match b {
            Block::Para { level, runs } => {
                let text: String = runs
                    .iter()
                    .map(|r| match (r.bold, r.italic) {
                        (true, true) => format!("***{}***", r.text),
                        (true, false) => format!("**{}**", r.text),
                        (false, true) => format!("*{}*", r.text),
                        _ => r.text.clone(),
                    })
                    .collect();
                if *level > 0 {
                    out += &format!("{} {}\n\n", "#".repeat(*level as usize), text);
                } else {
                    out += &format!("{text}\n\n");
                }
            }
            Block::Table(rows) => {
                for (i, row) in rows.iter().enumerate() {
                    out += &format!("| {} |\n", row.join(" | "));
                    if i == 0 {
                        out += &format!("|{}\n", " --- |".repeat(row.len()));
                    }
                }
                out += "\n";
            }
        }
    }
    out
}

fn esc_xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn blocks_to_docx(blocks: &[Block], out: &Path) -> Res<()> {
    let mut body = String::new();
    for b in blocks {
        match b {
            Block::Para { level, runs } => {
                body += "<w:p>";
                for r in runs {
                    let mut props = String::new();
                    if r.bold || *level > 0 {
                        props += "<w:b/>";
                    }
                    if r.italic {
                        props += "<w:i/>";
                    }
                    match level {
                        1 => props += "<w:sz w:val=\"40\"/>",
                        2 => props += "<w:sz w:val=\"32\"/>",
                        3 => props += "<w:sz w:val=\"28\"/>",
                        _ => {}
                    }
                    for (i, piece) in r.text.split('\n').enumerate() {
                        body += "<w:r>";
                        if !props.is_empty() {
                            body += &format!("<w:rPr>{props}</w:rPr>");
                        }
                        if i > 0 {
                            body += "<w:br/>";
                        }
                        body += &format!("<w:t xml:space=\"preserve\">{}</w:t></w:r>", esc_xml(piece));
                    }
                }
                body += "</w:p>";
            }
            Block::Table(rows) => {
                body += "<w:tbl><w:tblPr><w:tblBorders>";
                for side in ["top", "left", "bottom", "right", "insideH", "insideV"] {
                    body += &format!("<w:{side} w:val=\"single\" w:sz=\"4\" w:color=\"999999\"/>");
                }
                body += "</w:tblBorders></w:tblPr>";
                for row in rows {
                    body += "<w:tr>";
                    for cell in row {
                        body += &format!("<w:tc><w:p><w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p></w:tc>", esc_xml(cell));
                    }
                    body += "</w:tr>";
                }
                body += "</w:tbl><w:p/>";
            }
        }
    }
    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{body}<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1134\" w:right=\"1134\" w:bottom=\"1134\" w:left=\"1134\" w:header=\"708\" w:footer=\"708\" w:gutter=\"0\"/></w:sectPr></w:body></w:document>"
    );
    let content_types = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>";
    let rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>";

    let file = File::create(out).map_err(err)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, data) in [("[Content_Types].xml", content_types), ("_rels/.rels", rels), ("word/document.xml", document.as_str())] {
        zip.start_file(name, opts).map_err(err)?;
        zip.write_all(data.as_bytes()).map_err(err)?;
    }
    zip.finish().map_err(err)?;
    Ok(())
}

// ------------------------------------------------------------------ readers: file -> blocks

fn text_blocks(text: &str) -> Vec<Block> {
    text.replace("\r\n", "\n").lines().map(para).collect()
}

fn csv_rows(text: &str) -> Vec<Vec<String>> {
    text.replace("\r\n", "\n")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let mut cells = Vec::new();
            let mut cur = String::new();
            let mut quoted = false;
            let mut chars = line.chars().peekable();
            while let Some(c) = chars.next() {
                match c {
                    '"' if quoted && chars.peek() == Some(&'"') => {
                        cur.push('"');
                        chars.next();
                    }
                    '"' => quoted = !quoted,
                    ',' if !quoted => cells.push(std::mem::take(&mut cur)),
                    _ => cur.push(c),
                }
            }
            cells.push(cur);
            cells
        })
        .collect()
}

fn attr_value(e: &quick_xml::events::BytesStart, key: &[u8]) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == key)
        .and_then(|a| a.unescape_value().ok().map(|v| v.to_string()))
}

fn docx_blocks(path: &Path) -> Res<Vec<Block>> {
    let file = File::open(path).map_err(err)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| "This isn't a valid Word (.docx) file.".to_string())?;
    let mut xml = String::new();
    zip.by_name("word/document.xml")
        .map_err(|_| "This Word file has no readable text.".to_string())?
        .read_to_string(&mut xml)
        .map_err(err)?;

    let mut reader = Reader::from_str(&xml);
    let mut blocks = Vec::new();
    let (mut level, mut bold, mut italic, mut in_text) = (0u8, false, false, false);
    let mut runs: Vec<Run> = Vec::new();
    let mut table_depth = 0;
    let (mut rows, mut row, mut cell): (Vec<Vec<String>>, Vec<String>, String) = (Vec::new(), Vec::new(), String::new());

    loop {
        match reader.read_event().map_err(err)? {
            Event::Start(e) => match e.name().as_ref() {
                b"w:p" => {
                    runs.clear();
                    level = 0;
                }
                b"w:r" => {
                    bold = false;
                    italic = false;
                }
                b"w:t" => in_text = true,
                b"w:tbl" => {
                    table_depth += 1;
                    if table_depth == 1 {
                        rows.clear();
                    }
                }
                b"w:tr" => row.clear(),
                b"w:tc" => cell.clear(),
                _ => {}
            },
            Event::Empty(e) => match e.name().as_ref() {
                b"w:b" => bold = attr_value(&e, b"w:val").map(|v| v != "0" && v != "false").unwrap_or(true),
                b"w:i" => italic = attr_value(&e, b"w:val").map(|v| v != "0" && v != "false").unwrap_or(true),
                b"w:pStyle" => {
                    if let Some(v) = attr_value(&e, b"w:val") {
                        let v = v.to_lowercase();
                        if v == "title" {
                            level = 1;
                        } else if let Some(n) = v.strip_prefix("heading").and_then(|n| n.trim().parse::<u8>().ok()) {
                            level = n.clamp(1, 3);
                        }
                    }
                }
                b"w:br" | b"w:cr" => runs.push(plain("\n")),
                b"w:tab" => runs.push(plain("\t")),
                _ => {}
            },
            Event::Text(t) if in_text => {
                let text = t.unescape().map_err(err)?.to_string();
                runs.push(Run { text, bold, italic });
            }
            Event::End(e) => match e.name().as_ref() {
                b"w:t" => in_text = false,
                b"w:p" => {
                    if table_depth > 0 {
                        if !cell.is_empty() {
                            cell.push(' ');
                        }
                        cell += run_text(&runs).trim();
                    } else {
                        blocks.push(Block::Para { level, runs: std::mem::take(&mut runs) });
                    }
                }
                b"w:tc" => row.push(cell.trim().to_string()),
                b"w:tr" => rows.push(std::mem::take(&mut row)),
                b"w:tbl" => {
                    table_depth -= 1;
                    if table_depth == 0 {
                        blocks.push(Block::Table(std::mem::take(&mut rows)));
                    }
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(blocks)
}

fn md_blocks(text: &str) -> Vec<Block> {
    use pulldown_cmark::{Event as E, HeadingLevel, Options, Parser, Tag, TagEnd};
    let mut blocks = Vec::new();
    let mut runs: Vec<Run> = Vec::new();
    let (mut level, mut bold, mut italic, mut in_code) = (0u8, false, false, false);
    let mut table: Option<Vec<Vec<String>>> = None;
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();
    let flush = |blocks: &mut Vec<Block>, runs: &mut Vec<Run>, level: u8| {
        if !runs.is_empty() {
            blocks.push(Block::Para { level, runs: std::mem::take(runs) });
        }
    };
    for ev in Parser::new_ext(text, Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH) {
        match ev {
            E::Start(Tag::Heading { level: l, .. }) => {
                flush(&mut blocks, &mut runs, 0);
                level = match l {
                    HeadingLevel::H1 => 1,
                    HeadingLevel::H2 => 2,
                    _ => 3,
                };
            }
            E::End(TagEnd::Heading(_)) | E::End(TagEnd::Paragraph) | E::End(TagEnd::Item) => {
                flush(&mut blocks, &mut runs, level);
                level = 0;
            }
            E::Start(Tag::Item) => runs.push(plain("• ")),
            E::Start(Tag::Strong) => bold = true,
            E::End(TagEnd::Strong) => bold = false,
            E::Start(Tag::Emphasis) => italic = true,
            E::End(TagEnd::Emphasis) => italic = false,
            E::Start(Tag::CodeBlock(_)) => in_code = true,
            E::End(TagEnd::CodeBlock) => in_code = false,
            E::Start(Tag::Table(_)) => table = Some(Vec::new()),
            E::Start(Tag::TableCell) => cell.clear(),
            E::End(TagEnd::TableCell) => row.push(cell.trim().to_string()),
            E::End(TagEnd::TableHead) | E::End(TagEnd::TableRow) => {
                if let Some(t) = table.as_mut() {
                    t.push(std::mem::take(&mut row));
                }
            }
            E::End(TagEnd::Table) => {
                if let Some(t) = table.take() {
                    blocks.push(Block::Table(t));
                }
            }
            E::Text(t) | E::Code(t) => {
                if table.is_some() {
                    cell += &t;
                } else if in_code {
                    for line in t.lines() {
                        blocks.push(para(line));
                    }
                } else {
                    runs.push(Run { text: t.to_string(), bold, italic });
                }
            }
            E::SoftBreak => runs.push(plain(" ")),
            E::HardBreak => runs.push(plain("\n")),
            _ => {}
        }
    }
    flush(&mut blocks, &mut runs, 0);
    blocks
}

fn decode_entities(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// A forgiving HTML reader: headings, paragraphs, bold, italic and simple tables.
fn html_blocks(html: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut runs: Vec<Run> = Vec::new();
    let (mut level, mut bold, mut italic, mut skip) = (0u8, false, false, false);
    let mut table: Option<Vec<Vec<String>>> = None;
    let (mut row, mut cell): (Vec<String>, String) = (Vec::new(), String::new());
    let mut text = String::new();
    let mut chars = html.chars().peekable();

    macro_rules! push_text {
        () => {
            if !text.is_empty() {
                let t = decode_entities(&std::mem::take(&mut text));
                if table.is_some() {
                    cell += &t;
                } else if !skip {
                    runs.push(Run { text: t, bold, italic });
                }
            }
        };
    }
    macro_rules! end_block {
        () => {
            push_text!();
            if runs.iter().any(|r| !r.text.trim().is_empty()) {
                blocks.push(Block::Para { level, runs: std::mem::take(&mut runs) });
            } else {
                runs.clear();
            }
            level = 0;
        };
    }

    while let Some(c) = chars.next() {
        if c != '<' {
            if !skip {
                text.push(if c == '\n' || c == '\r' || c == '\t' { ' ' } else { c });
            }
            continue;
        }
        let mut tag = String::new();
        for t in chars.by_ref() {
            if t == '>' {
                break;
            }
            tag.push(t);
        }
        let closing = tag.starts_with('/');
        let name = tag.trim_start_matches('/').split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("").to_lowercase();
        match (name.as_str(), closing) {
            ("script" | "style" | "head", false) => skip = true,
            ("script" | "style" | "head", true) => skip = false,
            ("h1" | "h2" | "h3" | "h4" | "h5" | "h6", false) => {
                end_block!();
                level = name[1..].parse::<u8>().unwrap_or(1).min(3);
            }
            ("h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "p" | "div" | "li" | "section" | "article", true) => {
                end_block!();
            }
            ("p" | "div" | "section" | "article", false) => {
                end_block!();
            }
            ("li", false) => {
                end_block!();
                runs.push(plain("• "));
            }
            ("br", _) => {
                push_text!();
                if table.is_none() {
                    runs.push(plain("\n"));
                }
            }
            ("b" | "strong", false) => {
                push_text!();
                bold = true;
            }
            ("b" | "strong", true) => {
                push_text!();
                bold = false;
            }
            ("i" | "em", false) => {
                push_text!();
                italic = true;
            }
            ("i" | "em", true) => {
                push_text!();
                italic = false;
            }
            ("table", false) => {
                end_block!();
                table = Some(Vec::new());
            }
            ("td" | "th", false) => {
                push_text!();
                cell.clear();
            }
            ("td" | "th", true) => {
                push_text!();
                row.push(cell.trim().to_string());
            }
            ("tr", true) => {
                if let Some(t) = table.as_mut() {
                    t.push(std::mem::take(&mut row));
                }
            }
            ("table", true) => {
                if let Some(t) = table.take() {
                    blocks.push(Block::Table(t));
                }
            }
            _ => {}
        }
    }
    end_block!();
    blocks
}

fn read_text(path: &Path) -> Res<String> {
    let bytes = fs::read(path).map_err(err)?;
    // Windows files are sometimes UTF-16 or have a byte-order mark.
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let units: Vec<u16> = bytes[2..].chunks(2).filter(|c| c.len() == 2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        return Ok(String::from_utf16_lossy(&units));
    }
    let text = String::from_utf8_lossy(&bytes).to_string();
    Ok(text.trim_start_matches('\u{feff}').to_string())
}

fn doc_blocks(path: &Path, src: &str) -> Res<Vec<Block>> {
    Ok(match src {
        "docx" => docx_blocks(path)?,
        "md" | "markdown" => md_blocks(&read_text(path)?),
        "html" | "htm" => html_blocks(&read_text(path)?),
        "csv" => vec![Block::Table(csv_rows(&read_text(path)?))],
        _ => text_blocks(&read_text(path)?),
    })
}

// ------------------------------------------------------------------ writers

fn title_of(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
}

fn write_blocks(blocks: &[Block], target: &str, path: &Path) -> Res<Done> {
    let (stem, dir) = process::stem_dir(path);
    let out = process::unique(&dir, &stem, target);
    match target {
        "txt" => fs::write(&out, blocks_to_text(blocks)).map_err(err)?,
        "md" => fs::write(&out, blocks_to_md(blocks)).map_err(err)?,
        "html" => fs::write(&out, blocks_to_html(blocks, &title_of(path))).map_err(err)?,
        "docx" => blocks_to_docx(blocks, &out)?,
        other => return Err(format!("Can't convert to {other}.")),
    }
    Ok(Done { out: Some(out), message: String::new() })
}

// ------------------------------------------------------------------ HTML -> PDF with Edge

fn print_to_pdf(html_file: &Path, out: &Path) -> Res<()> {
    let edge = crate::system::edge_path().ok_or("Microsoft Edge is needed to make PDFs from documents, and it wasn't found.")?;
    let profile = std::env::temp_dir().join("halo-edge-profile");
    let url = format!("file:///{}", html_file.to_string_lossy().replace('\\', "/"));
    let mut child = Command::new(edge)
        .args(["--headless=new", "--disable-gpu", "--no-first-run", "--no-pdf-header-footer"])
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg(format!("--print-to-pdf={}", out.display()))
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(NO_WINDOW)
        .spawn()
        .map_err(err)?;
    let started = Instant::now();
    loop {
        if let Some(_status) = child.try_wait().map_err(err)? {
            break;
        }
        // Edge can linger after writing the file, so a finished PDF is enough.
        if out.exists() && fs::metadata(out).map(|m| m.len() > 0).unwrap_or(false) && started.elapsed() > Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(400));
            let _ = child.kill();
            break;
        }
        if started.elapsed() > Duration::from_secs(90) {
            let _ = child.kill();
            return Err("Making the PDF took too long.".into());
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    if fs::metadata(out).map(|m| m.len() > 0).unwrap_or(false) {
        Ok(())
    } else {
        Err("Edge couldn't create the PDF.".into())
    }
}

fn html_text_to_pdf(html: &str, out: &Path) -> Res<()> {
    let tmp = std::env::temp_dir().join(format!("halo-doc-{}.html", std::process::id()));
    fs::write(&tmp, html).map_err(err)?;
    let result = print_to_pdf(&tmp, out);
    let _ = fs::remove_file(&tmp);
    result
}

/// The document as HTML ready for printing.
fn doc_html(path: &Path, src: &str) -> Res<String> {
    match src {
        "html" | "htm" => read_text(path),
        "md" | "markdown" => {
            let text = read_text(path)?;
            let mut body = String::new();
            pulldown_cmark::html::push_html(&mut body, pulldown_cmark::Parser::new_ext(&text, pulldown_cmark::Options::ENABLE_TABLES | pulldown_cmark::Options::ENABLE_STRIKETHROUGH));
            Ok(wrap_html(&body, &title_of(path)))
        }
        _ => Ok(blocks_to_html(&doc_blocks(path, src)?, &title_of(path))),
    }
}

// ------------------------------------------------------------------ PDF

fn pdfium() -> Res<&'static Mutex<Pdfium>> {
    static PDFIUM: OnceLock<Result<Mutex<Pdfium>, String>> = OnceLock::new();
    PDFIUM
        .get_or_init(|| {
            // next to the app, in its resources folder, in src-tauri/resources while developing, or in Halo's downloaded tools
            for dir in crate::system::pdfium_dirs() {
                let lib = Pdfium::pdfium_platform_library_name_at_path(&dir);
                if lib.exists() {
                    return Pdfium::bind_to_library(&lib).map(|b| Mutex::new(Pdfium::new(b))).map_err(|e| format!("Couldn't start the PDF engine: {e}"));
                }
            }
            Err("The PDF engine (pdfium.dll) wasn't found next to Halo.".to_string())
        })
        .as_ref()
        .map_err(|e| e.clone())
}

fn pdf_error<E: std::fmt::Debug>(e: E) -> String {
    let text = format!("{e:?}");
    if text.to_lowercase().contains("password") {
        "This PDF is password protected.".to_string()
    } else {
        format!("Couldn't read this PDF ({text}).")
    }
}

fn pdf_pages(path: &Path, rep: &dyn Fn(f64, u64)) -> Res<Vec<DynamicImage>> {
    let pdfium = pdfium()?.lock().map_err(|_| "The PDF engine is busy.".to_string())?;
    let doc = pdfium.load_pdf_from_file(path, None).map_err(pdf_error)?;
    let total = doc.pages().len().max(1) as f64;
    let mut images = Vec::new();
    for (i, page) in doc.pages().iter().enumerate() {
        // A single full-page image is a scan/image PDF: retain its native pixels.
        let objects = page.objects();
        let native = if objects.len() == 1 && page.annotations().is_empty() && page.rotation().ok()==Some(PdfPageRenderRotation::None) {
            objects.iter().next().and_then(|object| {
                let bounds = object.bounds().ok()?;
                let image = object.as_image_object()?;
                let fills_page = bounds.left().value.abs() < 0.1 && bounds.bottom().value.abs() < 0.1
                    && (bounds.right().value - page.width().value).abs() < 0.1
                    && (bounds.top().value - page.height().value).abs() < 0.1;
                fills_page.then(|| image.get_processed_image(&doc).ok()).flatten()
            })
        } else { None };
        images.push(match native {
            Some(image) => image,
            None => page.render_with_config(&PdfRenderConfig::new().set_target_width(PAGE_WIDTH))
                .map_err(pdf_error)?.as_image().map_err(pdf_error)?,
        });
        rep(0.8 * (i as f64 + 1.0) / total, 0);
    }
    Ok(images)
}

/// Extract image objects independently of page size or surrounding text.
pub fn extract_images(path: &Path, rep: &dyn Fn(f64, u64)) -> Res<Done> {
    let engine = pdfium()?.lock().map_err(|_| "The PDF engine is busy.".to_string())?;
    let doc = engine.load_pdf_from_file(path, None).map_err(pdf_error)?;
    let mut images = Vec::new();
    for page in doc.pages().iter() {
        for object in page.objects().iter() {
            if let Some(image) = object.as_image_object() {
                images.push(image.get_processed_image(&doc).map_err(pdf_error)?);
            }
        }
    }
    if images.is_empty() { return Err("This PDF has no embedded images to extract.".into()); }
    let (stem, dir) = process::stem_dir(path);
    save_pages(&images, &format!("{stem}-images"), &dir, "png", rep)
}

fn pdf_blocks(path: &Path) -> Res<Vec<Block>> {
    let pdfium = pdfium()?.lock().map_err(|_| "The PDF engine is busy.".to_string())?;
    let doc = pdfium.load_pdf_from_file(path, None).map_err(pdf_error)?;
    let mut blocks = Vec::new();
    for page in doc.pages().iter() {
        let text = page.text().map_err(pdf_error)?.all();
        for line in text.replace("\r\n", "\n").lines() {
            if !line.trim().is_empty() {
                blocks.push(para(line.trim()));
            }
        }
    }
    if blocks.is_empty() {
        return Err("This PDF has no text to extract (it may be scanned pictures).".into());
    }
    Ok(blocks)
}

/// Saves each page as a picture: one file for a single page, otherwise a folder of numbered pictures.
fn save_pages(images: &[DynamicImage], stem: &str, dir: &Path, target: &str, rep: &dyn Fn(f64, u64)) -> Res<Done> {
    let ext = process::ext_for(target);
    if images.len() == 1 {
        let out = process::unique(dir, stem, ext);
        process::save_as(&images[0], target, &out).map_err(err)?;
        rep(1.0, process::size_of(&out));
        return Ok(Done { out: Some(out), message: String::new() });
    }
    let mut folder = dir.join(format!("{stem} pages"));
    let mut n = 1;
    while folder.exists() {
        folder = dir.join(format!("{stem} pages ({n})"));
        n += 1;
    }
    fs::create_dir_all(&folder).map_err(err)?;
    let width = images.len().to_string().len().max(2);
    for (i, img) in images.iter().enumerate() {
        let out = folder.join(format!("{stem}-page-{:0width$}.{ext}", i + 1));
        process::save_as(img, target, &out).map_err(err)?;
        rep(0.8 + 0.2 * (i as f64 + 1.0) / images.len() as f64, 0);
    }
    let name = folder.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    Ok(Done { out: Some(folder), message: format!("{} pages saved in the folder {name}", images.len()) })
}

fn pdf_convert(path: &Path, target: &str, rep: &dyn Fn(f64, u64)) -> Res<Done> {
    rep(0.05, 0);
    if IMAGE_TARGETS.contains(&target) {
        let images = pdf_pages(path, rep)?;
        let (stem, dir) = process::stem_dir(path);
        return save_pages(&images, &stem, &dir, target, rep);
    }
    match target {
        "txt" | "docx" | "md" | "html" => {
            let blocks = pdf_blocks(path)?;
            rep(0.7, 0);
            let done = write_blocks(&blocks, target, path)?;
            rep(1.0, 0);
            Ok(done)
        }
        other => Err(format!("Can't convert a PDF to {other}.")),
    }
}

// ------------------------------------------------------------------ documents

fn doc_convert(path: &Path, src: &str, target: &str, rep: &dyn Fn(f64, u64)) -> Res<Done> {
    rep(0.1, 0);
    let (stem, dir) = process::stem_dir(path);
    match target {
        "pdf" => {
            let html = doc_html(path, src)?;
            rep(0.4, 0);
            let out = process::unique(&dir, &stem, "pdf");
            html_text_to_pdf(&html, &out)?;
            rep(1.0, process::size_of(&out));
            Ok(Done { out: Some(out), message: String::new() })
        }
        "html" => {
            let html = doc_html(path, src)?;
            let out = process::unique(&dir, &stem, "html");
            fs::write(&out, html).map_err(err)?;
            Ok(Done { out: Some(out), message: String::new() })
        }
        "txt" | "md" | "docx" => {
            let blocks = doc_blocks(path, src)?;
            rep(0.6, 0);
            let done = write_blocks(&blocks, target, path)?;
            rep(1.0, 0);
            Ok(done)
        }
        t if IMAGE_TARGETS.contains(&t) => {
            // document -> PDF (in a temp folder) -> page pictures
            let html = doc_html(path, src)?;
            let tmp_pdf = std::env::temp_dir().join(format!("halo-doc-{}.pdf", std::process::id()));
            html_text_to_pdf(&html, &tmp_pdf)?;
            rep(0.4, 0);
            let images = pdf_pages(&tmp_pdf, &|p, b| rep(0.4 + p * 0.6, b));
            let _ = fs::remove_file(&tmp_pdf);
            save_pages(&images?, &stem, &dir, t, rep)
        }
        other => Err(format!("Can't convert a document to {other}.")),
    }
}

// ------------------------------------------------------------------ SVG

fn svg_convert(path: &Path, target: &str, rep: &dyn Fn(f64, u64)) -> Res<Done> {
    rep(0.1, 0);
    let data = fs::read(path).map_err(err)?;
    let mut opt = resvg::usvg::Options::default();
    opt.fontdb_mut().load_system_fonts();
    let tree = resvg::usvg::Tree::from_data(&data, &opt).map_err(|e| format!("This SVG can't be read ({e})."))?;
    let size = tree.size();
    // Small vector files would come out tiny, so draw them at a comfortable size.
    let scale = (1600.0 / size.width().max(size.height())).clamp(1.0, 8.0).max(1.0);
    let (w, h) = ((size.width() * scale).ceil() as u32, (size.height() * scale).ceil() as u32);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w.max(1), h.max(1)).ok_or("This SVG is too large to draw.")?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    rep(0.6, 0);
    let png = pixmap.encode_png().map_err(err)?;
    let img = image::load_from_memory(&png).map_err(err)?;

    let (stem, dir) = process::stem_dir(path);
    let out;
    if target == "pdf" {
        out = process::unique(&dir, &stem, "pdf");
        process::write_pdf(&img, &out)?;
    } else if IMAGE_TARGETS.contains(&target) {
        out = process::unique(&dir, &stem, process::ext_for(target));
        process::save_as(&img, target, &out)?;
    } else {
        return Err(format!("Can't convert an SVG to {target}."));
    }
    rep(1.0, process::size_of(&out));
    Ok(Done { out: Some(out), message: String::new() })
}
