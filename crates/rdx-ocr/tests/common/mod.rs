//! Shared by the tests that run the real OCR models.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

pub fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn fixture(name: &str) -> PathBuf {
    repo().join("fixtures/scans").join(name)
}

/// The models' directory, from the dev shell's `RDX_OCR_MODELS`. Read when the test is built,
/// so tests running as WebAssembly under wasmtime (which passes no environment) find it too.
pub fn models_dir() -> PathBuf {
    let dir = option_env!("RDX_OCR_MODELS").expect(
        "RDX_OCR_MODELS is not set: run the tests in the dev shell (`nix develop`), which \
         fetches the pinned OCR models, or point it at a directory holding text-detection.rten \
         and text-recognition.rten",
    );
    PathBuf::from(dir)
}

pub fn model_bytes() -> (Vec<u8>, Vec<u8>) {
    let dir = models_dir();
    (
        std::fs::read(dir.join("text-detection.rten")).unwrap(),
        std::fs::read(dir.join("text-recognition.rten")).unwrap(),
    )
}

pub fn models() -> rdx_ocr::Models {
    let (detection, recognition) = model_bytes();
    rdx_ocr::Models {
        detection,
        recognition,
    }
}
