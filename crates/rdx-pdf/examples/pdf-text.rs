//! Prints a PDF's text layer, spans separated by ` | `, for writing a layout parser against a
//! (synthetic or anonymized) sample: `cargo run -p rdx-pdf --example pdf-text -- FILE.pdf`.

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: pdf-text FILE.pdf");
        std::process::exit(2);
    };
    let bytes = std::fs::read(&path).unwrap_or_else(|e| {
        eprintln!("{path}: {e}");
        std::process::exit(2);
    });
    match rdx_pdf::text(&bytes) {
        Ok(text) => {
            for (i, page) in text.pages.iter().enumerate() {
                println!("--- page {}", i + 1);
                for line in &page.lines {
                    let spans: Vec<String> = line
                        .spans
                        .iter()
                        .map(|s| format!("{} [{:.0}-{:.0}]", s.text, s.x, s.x_end))
                        .collect();
                    println!("{:6.1}  {}", line.y, spans.join(" | "));
                }
            }
        }
        Err(e) => {
            eprintln!("{path}: {e}");
            std::process::exit(1);
        }
    }
}
