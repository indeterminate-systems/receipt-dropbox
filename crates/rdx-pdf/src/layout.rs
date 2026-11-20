//! Grouping placed glyphs into lines and spans.
//!
//! A line is glyphs sharing a baseline. Within a line, a gap wider than a space starts a new word;
//! a gap wider than [`SPAN_GAP`] font sizes starts a new span, which is how table columns come
//! apart.

use crate::content::Glyph;
use crate::{Line, Span};

/// Gap, in font sizes, above which two glyphs belong to different spans (table columns).
const SPAN_GAP: f64 = 1.0;
/// Gap, in font sizes, above which a space separates two glyphs.
const WORD_GAP: f64 = 0.15;
/// How far, in font sizes, a baseline may wander and still be the same line.
const BASELINE_TOLERANCE: f64 = 0.3;

fn round(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

pub(crate) fn lines(mut glyphs: Vec<Glyph>) -> Vec<Line> {
    glyphs.retain(|g| {
        !g.text.trim().is_empty() && g.size > 0.0 && g.x.is_finite() && g.y.is_finite()
    });
    // Top of the page first (PDF y grows upwards).
    glyphs.sort_by(|a, b| b.y.total_cmp(&a.y));

    let mut rows: Vec<(f64, Vec<Glyph>)> = Vec::new();
    for g in glyphs {
        match rows.last_mut() {
            Some((y, row))
                if (*y - g.y).abs() <= BASELINE_TOLERANCE * g.size.max(row_size(row)) =>
            {
                row.push(g)
            }
            _ => rows.push((g.y, vec![g])),
        }
    }

    rows.into_iter()
        .map(|(y, mut row)| {
            row.sort_by(|a, b| a.x.total_cmp(&b.x));
            Line {
                y: round(y),
                spans: spans(&row),
            }
        })
        .collect()
}

fn row_size(row: &[Glyph]) -> f64 {
    row.iter().map(|g| g.size).fold(0.0, f64::max)
}

fn spans(row: &[Glyph]) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let mut prev: Option<&Glyph> = None;
    for g in row {
        if let Some(p) = prev {
            let size = p.size.max(g.size);
            // Fake bold: the same glyph drawn twice, a fraction of a point apart.
            if p.text == g.text && (g.x - p.x).abs() < 0.2 * size {
                continue;
            }
            let gap = g.x - (p.x + p.width);
            let span = out.last_mut().expect("a previous glyph opened a span");
            if gap > SPAN_GAP * size {
                out.push(Span {
                    x: round(g.x),
                    x_end: round(g.x + g.width),
                    text: g.text.clone(),
                });
            } else {
                if gap > WORD_GAP * size {
                    span.text.push(' ');
                }
                span.text.push_str(&g.text);
                span.x_end = round(g.x + g.width);
            }
        } else {
            out.push(Span {
                x: round(g.x),
                x_end: round(g.x + g.width),
                text: g.text.clone(),
            });
        }
        prev = Some(g);
    }
    out
}
