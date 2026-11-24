//! The in-app path picker's model (TD-0009 A3.5 (12)): one folder's entries, listed off the UI
//! thread, over a [`TableModel`].
//!
//! The picker shows one folder at a time. Opening one ([`FolderModel::open`], Enter in the path
//! field, Enter on a folder's row, Up) starts a new [generation](Generation) and hands back a
//! [`ListRequest`] for a [`Lister`] to run on a thread of its own. Its entries come back in
//! [`ListBatch`]es of at most [`BATCH`], each tagged with the generation it lists, and
//! [`FolderModel::accept`] drops a batch of an older one, so a slow folder never paints over the
//! one opened after it. The UI thread never waits on the file system: whether a typed path
//! exists, and whether it's a folder, is the lister's answer too ([`ListError`]).
//!
//! - **One level.** A listing never recurses.
//! - **`..` is refused** before anything is listed ([`Refused`]), and so is a path without a root:
//!   the picker goes only where a full path says.
//! - **Rows.** Folders first, then (choosing a file) the files of the allowed [`Types`], each in
//!   name order, ignoring case. Choosing a folder lists no files.
//!
//! Slice 6e's MEASURE stub: the types are the seam the sheet track builds on. [`FolderModel`]
//! keeps a batch's entries in arrival order, unfiltered and unsorted, and refuses nothing;
//! [`FsLister`] lists nothing. The tests that need the real code are ignored until it lands.

use std::path::{Path, PathBuf};

use dpro_core::units::UnixMillis;

use crate::table::TableModel;

/// The most entries a [`ListBatch`] holds.
pub const BATCH: usize = 512;

/// Which listing a request or a batch belongs to: each [`FolderModel::open`] starts a new one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Generation(u32);

impl Generation {
    /// The generation after this one. Wraps: listings are only compared for equality.
    #[must_use]
    pub const fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

/// What the picker chooses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mode {
    /// A folder: the rows are its subfolders.
    Folder,
    /// A file of these types: the rows are the subfolders, then the matching files.
    File(Types),
}

/// The file types a file picker allows, from patterns as the dialog takes them (`*.wim;*.esd`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Types {
    /// Lowercase extensions, without their dot; empty for every file (`*.*`).
    extensions: Vec<Box<str>>,
}

impl Types {
    /// The types `patterns` name: each a list of `*.ext` separated by `;`. A `*` or `*.*`
    /// allows every file.
    #[must_use]
    pub fn new(patterns: &[&str]) -> Self {
        let _ = patterns;
        Self::default()
    }

    /// Whether a file named `name` is of one of these types (its extension, ignoring case).
    #[must_use]
    pub fn allows(&self, name: &str) -> bool {
        let _ = (name, &self.extensions);
        true
    }
}

/// One folder for a [`Lister`] to list, one level deep.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListRequest {
    pub generation: Generation,
    pub path: PathBuf,
}

/// One entry of a listed folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Its name in the folder, not its path.
    pub name: Box<str>,
    pub folder: bool,
    /// When it was last written; `None` if the file system didn't say.
    pub modified: Option<UnixMillis>,
}

/// Up to [`BATCH`] entries of one listing, in the lister's order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListBatch {
    pub generation: Generation,
    pub entries: Vec<Entry>,
    /// `None` while more batches follow; the listing's end on its last batch.
    pub end: Option<Result<(), ListError>>,
}

/// Why a folder couldn't be listed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ListError {
    /// Nothing is at the path (`ui-picker-not-found`).
    #[error("no such path")]
    NotFound,
    /// Something is, and it isn't a folder (`ui-picker-not-folder`).
    #[error("not a folder")]
    NotFolder,
    /// The folder can't be opened by this user (`ui-picker-denied`, in the error panel).
    #[error("access denied")]
    Denied,
    /// Anything else, with the system's error code if it gave one (`ui-picker-failed`).
    #[error("listing failed (os error {0:?})")]
    Failed(Option<i32>),
}

/// A path the picker won't list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Refused {
    /// It holds a `..` component (`ui-picker-refused`).
    #[error("the path holds `..`")]
    Parent,
    /// It has no root: a relative path, or none (`ui-picker-refused`).
    #[error("the path isn't a full path")]
    Relative,
}

/// Where the current listing stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Listing {
    /// Nothing opened yet.
    #[default]
    Idle,
    /// Batches may still come: the table shows what came, and `ui-loading` while it's empty.
    Loading,
    /// Every batch came: the table is the folder's (`ui-picker-empty-folders` or
    /// `ui-picker-empty-files` when it's empty).
    Listed,
    /// The listing ended in an error; the rows are what came before it.
    Failed(ListError),
}

/// What Enter on a row (or a double click) does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Activated {
    /// A folder's row opens it: run this request.
    Open(ListRequest),
    /// A file's row chooses it (choosing a file).
    Chose(PathBuf),
}

/// Runs [`ListRequest`]s off the UI thread and hands back their batches. The UI thread only ever
/// calls these, and neither waits.
pub trait Lister {
    /// Starts listing `request`. A listing still running is abandoned: what it sends later is
    /// stale, and dropped by [`FolderModel::accept`].
    fn start(&mut self, request: ListRequest);

    /// The next batch that came, if any; never waits.
    fn poll(&mut self) -> Option<ListBatch>;
}

/// Lists folders with the standard library, one listing at a time, each on a thread of its own:
/// batches of [`BATCH`], one level, a folder's attributes from its directory entry (no extra
/// call per entry). `wake` is called, on the listing's thread, whenever a batch is ready: the
/// host wakes the UI thread with it (`dpro_win::gui::Waker`), whose model then polls.
pub struct FsLister {
    wake: Box<dyn Fn() + Send + Sync>,
}

impl FsLister {
    #[must_use]
    pub fn new(wake: Box<dyn Fn() + Send + Sync>) -> Self {
        Self { wake }
    }
}

impl std::fmt::Debug for FsLister {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FsLister").finish_non_exhaustive()
    }
}

impl Lister for FsLister {
    fn start(&mut self, request: ListRequest) {
        let _ = (request, &self.wake);
    }

    fn poll(&mut self) -> Option<ListBatch> {
        None
    }
}

/// The picker's state on the UI thread: the path field's text, the folder shown and its rows.
#[derive(Debug)]
pub struct FolderModel {
    mode: Mode,
    /// The path field's text, as typed or as the last folder opened.
    path: String,
    /// The folder listed, or being listed.
    folder: Option<PathBuf>,
    generation: Generation,
    listing: Listing,
    /// The rows: the folders, then the files.
    entries: Vec<Entry>,
    table: TableModel,
}

impl FolderModel {
    /// A picker choosing as `mode` says, with nothing opened.
    #[must_use]
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            path: String::new(),
            folder: None,
            generation: Generation::default(),
            listing: Listing::Idle,
            entries: Vec::new(),
            table: TableModel::new(0),
        }
    }

    #[must_use]
    pub fn mode(&self) -> &Mode {
        &self.mode
    }

    /// The path field's text.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The path field's text, to edit.
    pub fn path_mut(&mut self) -> &mut String {
        &mut self.path
    }

    /// The folder shown (or being listed); `None` before the first open.
    #[must_use]
    pub fn folder(&self) -> Option<&Path> {
        self.folder.as_deref()
    }

    /// The current listing's generation.
    #[must_use]
    pub fn generation(&self) -> Generation {
        self.generation
    }

    #[must_use]
    pub fn listing(&self) -> Listing {
        self.listing
    }

    /// Opens `path`: a new generation, no rows, [`Listing::Loading`], the path field showing it.
    ///
    /// # Errors
    /// [`Refused`] if `path` holds `..` or has no root; nothing changes but the field's text.
    pub fn open(&mut self, path: &Path) -> Result<ListRequest, Refused> {
        self.generation = self.generation.next();
        self.folder = Some(path.to_path_buf());
        self.path = path.to_string_lossy().into_owned();
        self.listing = Listing::Loading;
        self.entries.clear();
        self.table = TableModel::new(0);
        Ok(ListRequest {
            generation: self.generation,
            path: path.to_path_buf(),
        })
    }

    /// Enter in the path field: opens what it says.
    ///
    /// # Errors
    /// As [`open`](Self::open).
    pub fn go(&mut self) -> Result<ListRequest, Refused> {
        let path = PathBuf::from(&self.path);
        self.open(&path)
    }

    /// Up one level (Up, Alt+Up, Backspace in the table): opens the shown folder's parent.
    /// `None` at a root, or before the first open.
    pub fn up(&mut self) -> Option<ListRequest> {
        None
    }

    /// Enter on the row at `row` (source order): opens a folder, or chooses a file. `None` for
    /// no such row.
    pub fn activate(&mut self, row: u32) -> Option<Activated> {
        let _ = row;
        None
    }

    /// Takes a batch: its entries join the rows, in their places, if it's the current
    /// generation's. Returns whether it was; a stale batch changes nothing.
    pub fn accept(&mut self, batch: ListBatch) -> bool {
        if batch.generation != self.generation || self.listing != Listing::Loading {
            return false;
        }
        self.entries.extend(batch.entries);
        match batch.end {
            None => {}
            Some(Ok(())) => self.listing = Listing::Listed,
            Some(Err(e)) => self.listing = Listing::Failed(e),
        }
        self.table = TableModel::new(self.len());
        true
    }

    /// How many rows there are.
    #[must_use]
    pub fn len(&self) -> u32 {
        u32::try_from(self.entries.len()).unwrap_or(u32::MAX)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The row at `row`, in source order: the folders, then the files.
    #[must_use]
    pub fn entry(&self, row: u32) -> Option<&Entry> {
        self.entries.get(usize::try_from(row).ok()?)
    }

    /// The rows' view, current row and selection.
    #[must_use]
    pub fn table(&self) -> &TableModel {
        &self.table
    }

    pub fn table_mut(&mut self) -> &mut TableModel {
        &mut self.table
    }

    /// What Select chooses: choosing a folder, the shown folder; choosing a file, the current
    /// row's file. `None` if there's nothing to choose (Select is disabled).
    #[must_use]
    pub fn choice(&self) -> Option<PathBuf> {
        None
    }
}
