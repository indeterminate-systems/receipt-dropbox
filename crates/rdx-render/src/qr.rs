//! The envelope as a QR code, drawn as SVG for Typst to place. Typst has QR packages, but the core
//! fetches no packages, and the code is a few lines given `qrcode`'s module matrix.

use qrcode::{Color, EcLevel, QrCode};

/// Modules of light border around the code, as the QR specification asks.
const QUIET: usize = 4;

/// An SVG QR code of `ur`, uppercased (URs are case-insensitive, and uppercase fits QR's compact
/// alphanumeric mode), or `None` when even the largest code can't hold it. Medium error
/// correction survives a smudged printout; low is the fallback for envelopes just too big for it.
pub fn svg(ur: &str) -> Option<String> {
    let data = ur.trim().to_ascii_uppercase();
    let code = [EcLevel::M, EcLevel::L]
        .into_iter()
        .find_map(|level| QrCode::with_error_correction_level(&data, level).ok())?;
    let width = code.width();
    let size = width + 2 * QUIET;
    let mut path = String::new();
    for (i, color) in code.to_colors().into_iter().enumerate() {
        if color == Color::Dark {
            let (x, y) = (i % width + QUIET, i / width + QUIET);
            path.push_str(&format!("M{x} {y}h1v1h-1z"));
        }
    }
    Some(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {size} {size}\" \
         shape-rendering=\"crispEdges\"><rect width=\"{size}\" height=\"{size}\" fill=\"#fff\"/>\
         <path d=\"{path}\" fill=\"#000\"/></svg>"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_a_square_code_with_a_quiet_zone() {
        let svg = svg("ur:bytes/hdcx").unwrap();
        assert!(svg.starts_with("<svg") && svg.contains("<path d=\"M"));
        // Version 1 is 21 modules wide; with 4 on each side, 29.
        assert!(svg.contains("viewBox=\"0 0 29 29\""), "{svg}");
    }

    #[test]
    fn gives_up_beyond_the_largest_code() {
        // Version 40 holds 4,296 alphanumeric characters at low error correction.
        assert!(svg(&format!("ur:envelope/{}", "a".repeat(4300))).is_none());
    }
}
