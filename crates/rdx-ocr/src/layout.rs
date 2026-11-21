//! Words OCR found, grouped into the same lines and spans the PDF text layer gives, with
//! positions in points, so parsers that read columns (`example-bank`) work on scans unchanged.

use rdx_pdf::{Line, Span};

/// A recognized word: its box in image pixels (y grows downwards) and its text.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Word {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub text: String,
}

impl Word {
    fn height(&self) -> f32 {
        (self.bottom - self.top).max(1.0)
    }
    fn middle(&self) -> f32 {
        (self.top + self.bottom) / 2.0
    }
}

/// Where an image sits on its page: pixels to points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Frame {
    /// Left edge of the image, in points from the page's left.
    pub x: f64,
    /// Top edge of the image, in points from the page's bottom.
    pub top: f64,
    /// Points per pixel, across and down.
    pub sx: f64,
    pub sy: f64,
}

/// Two words whose middles are closer than this many word heights share a line.
const SAME_LINE: f32 = 0.5;
/// A gap wider than this many word heights starts a new span (a table column). Text is about
/// one word height per character, the PDF layout's `SPAN_GAP` of one font size.
const SPAN_GAP: f32 = 1.0;

fn round(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

pub(crate) fn lines(mut words: Vec<Word>, frame: Frame) -> Vec<Line> {
    words.retain(|w| !w.text.trim().is_empty());
    words.sort_by(|a, b| a.middle().total_cmp(&b.middle()));

    let mut rows: Vec<Vec<Word>> = Vec::new();
    for w in words {
        match rows.last_mut() {
            Some(row) if same_line(row, &w) => row.push(w),
            _ => rows.push(vec![w]),
        }
    }

    rows.into_iter()
        .map(|mut row| {
            row.sort_by(|a, b| a.left.total_cmp(&b.left));
            // The baseline is near the bottom of the words, which most characters sit on.
            let bottom = median(row.iter().map(|w| w.bottom));
            Line {
                y: round(frame.top - f64::from(bottom) * frame.sy),
                spans: spans(&row, frame),
            }
        })
        .collect()
}

fn same_line(row: &[Word], w: &Word) -> bool {
    let middle = median(row.iter().map(Word::middle));
    let height = median(row.iter().map(Word::height)).max(w.height());
    (w.middle() - middle).abs() <= SAME_LINE * height
}

fn spans(row: &[Word], frame: Frame) -> Vec<Span> {
    let height = median(row.iter().map(Word::height));
    let x = |px: f32| round(frame.x + f64::from(px) * frame.sx);
    let mut out: Vec<Span> = Vec::new();
    let mut prev_right = f32::NEG_INFINITY;
    for w in row {
        match out.last_mut() {
            Some(span) if w.left - prev_right <= SPAN_GAP * height => {
                span.text.push(' ');
                span.text.push_str(&w.text);
                span.x_end = x(w.right);
            }
            _ => out.push(Span {
                x: x(w.left),
                x_end: x(w.right),
                text: w.text.clone(),
            }),
        }
        prev_right = prev_right.max(w.right);
    }
    out
}

fn median(values: impl Iterator<Item = f32>) -> f32 {
    let mut v: Vec<f32> = values.collect();
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f32::total_cmp);
    v[v.len() / 2]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(left: f32, top: f32, right: f32, text: &str) -> Word {
        Word {
            left,
            top,
            right,
            bottom: top + 20.0,
            text: text.into(),
        }
    }

    /// 200 pixels per inch, image covering a US Letter page from its top left.
    const LETTER_200: Frame = Frame {
        x: 0.0,
        top: 792.0,
        sx: 72.0 / 200.0,
        sy: 72.0 / 200.0,
    };

    #[test]
    fn words_become_lines_top_to_bottom_and_columns_become_spans() {
        let words = vec![
            // Second row, slightly lower on the right, as a skewed scan would be.
            w(150.0, 530.0, 205.0, "03/03"),
            w(245.0, 527.0, 380.0, "LANTERN"),
            w(390.0, 527.0, 450.0, "CAFE"),
            w(1224.0, 531.0, 1284.0, "289.74"),
            w(1489.0, 532.0, 1547.0, "454.81"),
            // First row.
            w(150.0, 495.0, 202.0, "Date"),
            w(1463.0, 495.0, 1549.0, "Balance"),
        ];
        let lines = lines(words, LETTER_200);
        assert_eq!(lines.len(), 2);
        let texts: Vec<Vec<&str>> = lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.text.as_str()).collect())
            .collect();
        assert_eq!(texts[0], ["Date", "Balance"]);
        assert_eq!(texts[1], ["03/03", "LANTERN CAFE", "289.74", "454.81"]);
        // 1547 px at 200 ppi is 556.9 pt; rows read downwards, so y falls.
        assert_eq!(lines[1].spans[3].x_end, 556.9);
        assert!(lines[0].y > lines[1].y);
        assert_eq!(lines[0].y, 792.0 - 515.0 * 0.36);
    }

    #[test]
    fn a_frame_places_an_image_that_does_not_fill_the_page() {
        let frame = Frame {
            x: 72.0,
            top: 700.0,
            sx: 0.5,
            sy: 0.5,
        };
        let lines = lines(vec![w(10.0, 0.0, 30.0, "Total")], frame);
        assert_eq!(lines[0].spans[0].x, 77.0);
        assert_eq!(lines[0].spans[0].x_end, 87.0);
        assert_eq!(lines[0].y, 690.0);
    }
}
