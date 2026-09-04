//! Locate requirement blocks via acdc, then re-scan their attribute lines.
//!
//! acdc's AST carries byte-accurate `Location` spans on every block, including
//! `open_delimiter_location` / `close_delimiter_location` on delimited blocks.
//! It does not carry the *attribute lines* above a block as part of the block's
//! span, so those are recovered by scanning the lines immediately above the
//! open delimiter — the narrowest gap-filler; the block itself is never
//! reserialized.

use acdc_parser::{Block, DelimitedBlock, DelimitedBlockType, parse};
use serde::{Deserialize, Serialize};

use crate::{AttrLine, Error, Item, Record, Span};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Extraction {
    pub file: String,
    pub records: Vec<Record>,
}

/// `extract`: every requirement block in `source`, in source order.
pub fn extract(file: &str, source: &str) -> crate::Result<Vec<Record>> {
    let doc =
        parse(source, &Default::default()).map_err(|e| Error::Parse(format!("{file}: {e}")))?;
    let doc = doc.document();
    let mut spans = Vec::new();
    collect_requirement_spans(&doc.blocks, &mut spans);
    spans.dedup();

    let lines = line_table(source);
    let mut records = Vec::new();
    for span in spans {
        let start_line = line_index_of(&lines, span.absolute_start)
            .ok_or_else(|| Error::Parse(format!("{file}: block start out of range")))?;
        // acdc's block `Location` begins at the first stacked attribute line,
        // not at the delimiter. Attribute lines are the leading run of
        // `[...]` lines; the next line is the open delimiter.
        let mut last_attr = start_line;
        while lines
            .get(last_attr + 1)
            .is_some_and(|&l| is_attr_line(l, source))
        {
            last_attr += 1;
        }
        #[expect(
            clippy::indexing_slicing,
            reason = "start_line..=last_attr are valid line-table indices"
        )]
        let attr_lines: Vec<AttrLine> = lines[start_line..=last_attr]
            .iter()
            .map(|&(s, e)| parse_attr_line(source, s, e))
            .collect();
        let id = attr_lines
            .first()
            .and_then(|l| {
                l.items.iter().find_map(|i| match i {
                    Item::Id(id) => Some(id.clone()),
                    _ => None,
                })
            })
            .ok_or_else(|| {
                Error::Parse(format!("{file}: requirement block without an id anchor"))
            })?;
        let open_delim_line = last_attr + 1;
        #[expect(
            clippy::string_slice,
            reason = "line-table ends fall on \n or the \r before it, always char boundaries"
        )]
        #[expect(
            clippy::indexing_slicing,
            reason = "open_delim_line is a valid line-table index by construction"
        )]
        let delim = source[lines[open_delim_line].0..lines[open_delim_line].1]
            .trim()
            .to_owned();
        let close_line_idx = line_index_of(&lines, span.absolute_end)
            .ok_or_else(|| Error::Parse(format!("{file}: unterminated requirement block")))?;
        #[expect(
            clippy::indexing_slicing,
            reason = "close_line_idx is a valid line-table index"
        )]
        let body_end = lines[close_line_idx].0;
        let body_start = lines.get(open_delim_line + 1).map(|&(s, _)| s).unwrap_or(0);
        #[expect(
            clippy::string_slice,
            reason = "body bounds are line-table starts, always char boundaries"
        )]
        let body_raw = source[body_start..body_end].to_owned();
        records.push(Record {
            file: file.to_owned(),
            id,
            span: Span {
                #[expect(
                    clippy::indexing_slicing,
                    reason = "start_line is a valid line-table index"
                )]
                start: lines[start_line].0,
                end: span.absolute_end + 1,
            },
            attr_lines,
            delim,
            body_raw,
        });
    }
    Ok(records)
}

fn collect_requirement_spans<'a>(blocks: &[Block<'a>], out: &mut Vec<acdc_parser::Location>) {
    for block in blocks {
        match block {
            Block::Section(s) => collect_requirement_spans(&s.content, out),
            Block::DelimitedBlock(d) if d.metadata.style == Some("requirement") => {
                collect_inner(&d.inner, out);
                out.push(d.location.clone());
            }
            Block::DelimitedBlock(d) => collect_inner(&d.inner, out),
            _ => {}
        }
    }
}

fn collect_inner(inner: &DelimitedBlockType<'_>, out: &mut Vec<acdc_parser::Location>) {
    if let DelimitedBlockType::DelimitedOpen(inner) | DelimitedBlockType::DelimitedExample(inner) =
        inner
    {
        collect_requirement_spans(inner, out);
    }
}

/// `(start, end)` byte range per line, excluding line terminators.
fn line_table(source: &str) -> Vec<(usize, usize)> {
    let mut table = Vec::new();
    let mut start = 0;
    for line in source.split('\n') {
        let end = start + line.len();
        table.push((start, if line.ends_with('\r') { end - 1 } else { end }));
        start = end + 1;
    }
    table
}

fn line_index_of(lines: &[(usize, usize)], offset: usize) -> Option<usize> {
    lines.iter().position(|&(s, e)| offset >= s && offset <= e)
}

fn is_attr_line((s, e): (usize, usize), source: &str) -> bool {
    #[expect(
        clippy::string_slice,
        reason = "line-table ends fall on \n or the \r before it, always char boundaries"
    )]
    let line = source[s..e].trim();
    line.starts_with('[') && line.ends_with(']')
}

/// Parse one `[...]` line into items. Quoted values may contain commas.
pub fn parse_attr_line(source: &str, start: usize, end: usize) -> AttrLine {
    #[expect(
        clippy::string_slice,
        reason = "line-table ends fall on \n or the \r before it, always char boundaries"
    )]
    let raw = source[start..end].to_owned();
    let items = parse_items(raw.trim());
    AttrLine {
        start,
        end,
        raw,
        items,
    }
}

fn parse_items(line: &str) -> Vec<Item> {
    let inner = line.trim().trim_start_matches('[').trim_end_matches(']');
    split_top_level(inner)
        .into_iter()
        .filter(|s| !s.trim().is_empty())
        .flat_map(|s| {
            let s = s.trim();
            if let Some(id) = s.strip_prefix('#') {
                vec![Item::Id(id.to_owned())]
            } else if let Some((style, id)) = s.split_once('#') {
                // AsciiDoc shorthand `[style#id]`: style and id share one item.
                let mut items = Vec::new();
                if !style.is_empty() {
                    items.push(Item::Style(style.to_owned()));
                }
                items.push(Item::Id(id.to_owned()));
                items
            } else if let Some((name, value)) = s.split_once('=') {
                vec![Item::Kv {
                    name: name.trim().to_owned(),
                    value: unquote(value.trim()),
                }]
            } else {
                vec![Item::Style(s.to_owned())]
            }
        })
        .collect()
}

fn split_top_level(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for c in s.chars() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
                current.push(c);
            }
            ',' if !in_quotes => parts.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    parts.push(current);
    parts
}

fn unquote(s: &str) -> String {
    s.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(s)
        .to_owned()
}

/// Render items back into an attribute line. Values containing `,`, `]`, or
/// `"` are quoted.
pub fn render_attr_line(items: &[Item]) -> String {
    let mut out = String::from("[");
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        match item {
            Item::Style(s) => out.push_str(s),
            Item::Id(id) => {
                out.push('#');
                out.push_str(id);
            }
            Item::Kv { name, value } => {
                out.push_str(name);
                out.push('=');
                if value.contains(',') || value.contains(']') || value.contains('"') {
                    out.push('"');
                    out.push_str(value);
                    out.push('"');
                } else {
                    out.push_str(value);
                }
            }
            Item::Raw(r) => out.push_str(r),
        }
    }
    out.push(']');
    out
}

/// Expose the delimiter shape acdc saw, for tests and the spec.
pub fn block_kind(d: &DelimitedBlock<'_>) -> &'static str {
    match &d.inner {
        DelimitedBlockType::DelimitedOpen(_) => "open",
        DelimitedBlockType::DelimitedExample(_) => "example",
        _ => "other",
    }
}
