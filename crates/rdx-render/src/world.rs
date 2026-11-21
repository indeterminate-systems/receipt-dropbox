//! The smallest Typst world: one source file, fonts compiled in, nothing else. Typst asks the world
//! for files, packages and the date; this one has none of them, so a render depends only on its
//! data and can't reach a file system or network that the core doesn't have.

use std::sync::OnceLock;

use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime, Dict, Duration, IntoValue};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};

struct Fonts {
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
}

/// Parsing the embedded fonts takes a moment; do it once per process.
fn fonts() -> &'static Fonts {
    static FONTS: OnceLock<Fonts> = OnceLock::new();
    FONTS.get_or_init(|| {
        let fonts: Vec<Font> = typst_assets::fonts()
            .flat_map(|data| Font::iter(Bytes::new(data)))
            .collect();
        Fonts {
            book: LazyHash::new(FontBook::from_fonts(&fonts)),
            fonts,
        }
    })
}

pub struct StatementWorld {
    library: LazyHash<Library>,
    main: Source,
}

impl StatementWorld {
    /// `template` is Typst source; `data` is what it reads as `sys.inputs.data`.
    pub fn new(template: &str, data: String) -> Self {
        let mut inputs = Dict::new();
        inputs.insert("data".into(), data.into_value());
        let id = RootedPath::new(
            VirtualRoot::Project,
            VirtualPath::new("/statement.typ").expect("a valid virtual path"),
        )
        .intern();
        Self {
            library: LazyHash::new(Library::builder().with_inputs(inputs).build()),
            main: Source::new(id, template.to_owned()),
        }
    }
}

impl World for StatementWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &fonts().book
    }

    fn main(&self) -> FileId {
        self.main.id()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main.id() {
            Ok(self.main.clone())
        } else {
            Err(FileError::AccessDenied)
        }
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        if id == self.main.id() {
            Ok(Bytes::from_string(self.main.clone()))
        } else {
            Err(FileError::AccessDenied)
        }
    }

    fn font(&self, index: usize) -> Option<Font> {
        fonts().fonts.get(index).cloned()
    }

    /// No clock: a rendering must not depend on when it was made.
    fn today(&self, _: Option<Duration>) -> Option<Datetime> {
        None
    }
}
