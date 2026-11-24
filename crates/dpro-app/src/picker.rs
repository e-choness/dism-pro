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
//! - **One level.** A listing never recurses, so a junction is listed only when it's opened.
//! - **`..` is refused** before anything is listed ([`Refused`]), and so is a path without a root:
//!   the picker goes only where a full path says. `\\?\` paths and UNC paths are full paths.
//! - **Rows.** Folders first, then (choosing a file) the files of the allowed [`Types`], each in
//!   name order, ignoring case. Choosing a folder lists no files. Hidden entries are listed: a
//!   servicing tool's user often wants `ProgramData` or a `$`-folder.
//! - **Untrusted names** (A3.8). A name or path is shown as [`display_text`] shows it, its control
//!   and bidirectional characters as escapes (`<U+202E>`), so a folder can't disguise what it
//!   is. A [`Lister`] hands names so, made on its own thread (A3.7: the picker's O(entries) work
//!   is off the UI thread), and the path field shows the folder so. An escape keeps every other
//!   character where it was, so a name's extension is the real one's for the file types. Opening
//!   a row, choosing a file and going where the field says turn the escapes back first
//!   ([`real_text`]), so the path listed or chosen is the real one.

use std::{
    cmp::Ordering,
    fs,
    io::{self, ErrorKind},
    mem,
    path::{Component, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering as Atomic},
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::UNIX_EPOCH,
};

use dpro_core::units::UnixMillis;

use crate::{
    display::{display_text, push_display, real_text},
    table::TableModel,
};

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
    /// Extensions, lowercase per character, without their dot; empty for every file (`*.*`).
    extensions: Vec<Box<str>>,
}

impl Types {
    /// The types `patterns` name: each a list of `*.ext` separated by `;`. A `*` or `*.*`
    /// allows every file, as no patterns do. `.ext` and `ext` mean `*.ext`; a pattern with a
    /// wildcard inside (`install*.wim`) matches no name.
    #[must_use]
    pub fn new(patterns: &[&str]) -> Self {
        let mut extensions: Vec<Box<str>> = Vec::new();
        let mut every = false;
        for pattern in patterns.iter().flat_map(|p| p.split(';')) {
            let pattern = pattern.trim();
            if pattern.is_empty() {
                continue;
            }
            let ext = pattern.strip_prefix('*').unwrap_or(pattern);
            let ext = ext.strip_prefix('.').unwrap_or(ext);
            if ext.is_empty() || ext == "*" {
                every = true;
                continue;
            }
            let mut lower = String::with_capacity(ext.len());
            crate::fold::lower_into(ext, &mut lower);
            if !extensions.iter().any(|e| **e == *lower) {
                extensions.push(lower.into_boxed_str());
            }
        }
        if every {
            extensions.clear();
        }
        Self { extensions }
    }

    /// Whether a file named `name` is of one of these types: its name ends in a dot and one of
    /// the extensions, ignoring case (per character, as the extensions were lowered). Nothing
    /// is allocated.
    #[must_use]
    pub fn allows(&self, name: &str) -> bool {
        self.extensions.is_empty()
            || self.extensions.iter().any(|ext| {
                // From the end: the name's characters lowered, against the extension's.
                let mut tail = name.chars().rev().flat_map(|c| c.to_lowercase().rev());
                ext.chars().rev().all(|e| tail.next() == Some(e)) && tail.next() == Some('.')
            })
    }
}

/// One folder for a [`Lister`] to list, one level deep.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListRequest {
    /// The listing this request starts: its batches carry it.
    pub generation: Generation,
    /// The folder to list.
    pub path: PathBuf,
}

/// One entry of a listed folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Its name in the folder, not its path, as [`display_text`] shows it (the lister made it
    /// so). A name that isn't valid Unicode shows with U+FFFD where it isn't, and then can't be
    /// opened.
    pub name: Box<str>,
    /// A folder, or a link to one (a junction, a folder symbolic link): opening it lists its
    /// target, one level.
    pub folder: bool,
    /// When it was last written; `None` if the file system didn't say.
    pub modified: Option<UnixMillis>,
}

/// Up to [`BATCH`] entries of one listing, in the lister's order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListBatch {
    /// The listing it belongs to: the request's.
    pub generation: Generation,
    /// Its entries, at most [`BATCH`].
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
    /// Anything else, with the system's error code if it gave one (`ui-picker-failed`; the app
    /// decodes the code: device not ready, and so on).
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

/// Runs [`ListRequest`]s off the UI thread and hands back their batches, each entry's name as
/// [`display_text`] shows it. The UI thread only ever calls these, and neither waits.
pub trait Lister {
    /// Starts listing `request`. A listing still running is abandoned: what it sends later is
    /// stale, and dropped by [`FolderModel::accept`].
    fn start(&mut self, request: ListRequest);

    /// The next batch that came, if any; never waits.
    fn poll(&mut self) -> Option<ListBatch>;
}

/// Lists folders with the standard library, each listing on a thread of its own: batches of
/// [`BATCH`], one level, an entry's kind and time from its directory entry (on Windows, no call
/// per entry). `wake` is called, on the listing's thread, whenever a batch is ready: the host
/// wakes the UI thread with it (`dpro_win::gui::Waker`), whose model then polls.
///
/// A listing that hangs (a dead network share, a damaged disk) holds only its own thread: a new
/// request starts another. Once a newer listing has started, an older one sends nothing more and
/// stops at its next batch.
pub struct FsLister {
    wake: Arc<dyn Fn() + Send + Sync>,
    /// The newest request's generation: an older listing's thread stops at its next batch.
    newest: Arc<AtomicU32>,
    send: Sender<ListBatch>,
    batches: Receiver<ListBatch>,
}

impl FsLister {
    /// A lister that calls `wake` on a listing's thread after each batch it sends. Starts no
    /// thread until a request comes; changes nothing on disk.
    #[must_use]
    pub fn new(wake: Box<dyn Fn() + Send + Sync>) -> Self {
        let (send, batches) = mpsc::channel();
        Self {
            wake: Arc::from(wake),
            newest: Arc::new(AtomicU32::new(0)),
            send,
            batches,
        }
    }
}

impl std::fmt::Debug for FsLister {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FsLister")
            .field("newest", &self.newest)
            .finish_non_exhaustive()
    }
}

impl Lister for FsLister {
    fn start(&mut self, request: ListRequest) {
        let generation = request.generation;
        self.newest.store(generation.0, Atomic::Release);
        let job = ListJob {
            request,
            send: self.send.clone(),
            wake: Arc::clone(&self.wake),
            newest: Arc::clone(&self.newest),
        };
        let spawned = thread::Builder::new()
            .name("dpro-picker-list".into())
            .spawn(move || job.run());
        if let Err(e) = spawned {
            // No thread to list it: the listing ends at once, in the error, so the picker says
            // so rather than load forever.
            let batch = ListBatch {
                generation,
                entries: Vec::new(),
                end: Some(Err(ListError::Failed(e.raw_os_error()))),
            };
            if self.send.send(batch).is_ok() {
                (self.wake)();
            }
        }
    }

    fn poll(&mut self) -> Option<ListBatch> {
        self.batches.try_recv().ok()
    }
}

/// One listing, on its own thread.
struct ListJob {
    request: ListRequest,
    send: Sender<ListBatch>,
    wake: Arc<dyn Fn() + Send + Sync>,
    newest: Arc<AtomicU32>,
}

impl ListJob {
    fn run(self) {
        let path = self.request.path.as_path();
        let dir = match fs::read_dir(path) {
            Ok(dir) => dir,
            Err(e) => {
                self.send(Vec::new(), Some(Err(opening_error(path, &e))));
                return;
            }
        };
        let mut batch = Vec::with_capacity(BATCH);
        let mut end = Ok(());
        for item in dir {
            match item {
                Ok(item) => batch.push(entry(&item)),
                Err(e) => {
                    end = Err(error(&e));
                    break;
                }
            }
            if batch.len() == BATCH
                && !self.send(mem::replace(&mut batch, Vec::with_capacity(BATCH)), None)
            {
                return;
            }
        }
        self.send(batch, Some(end));
    }

    /// Sends a batch and wakes the UI thread. `false` once a newer listing has started or the
    /// picker is gone: then the listing stops.
    fn send(&self, entries: Vec<Entry>, end: Option<Result<(), ListError>>) -> bool {
        let generation = self.request.generation;
        if self.newest.load(Atomic::Acquire) != generation.0 {
            return false;
        }
        let batch = ListBatch {
            generation,
            entries,
            end,
        };
        let sent = self.send.send(batch).is_ok();
        if sent {
            (self.wake)();
        }
        sent
    }
}

/// An entry from its directory entry: on Windows its kind and time come with it.
fn entry(item: &fs::DirEntry) -> Entry {
    let folder = item
        .file_type()
        .is_ok_and(|kind| kind.is_dir() || (kind.is_symlink() && linked_folder(item, kind)));
    let modified = item
        .metadata()
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| UnixMillis(u64::try_from(d.as_millis()).unwrap_or(u64::MAX)));
    Entry {
        name: display_text(&item.file_name().to_string_lossy()).into(),
        folder,
        modified,
    }
}

/// Whether a link is to a folder. On Windows its own attributes say (a junction or a folder
/// symbolic link carries the folder attribute), with no call.
#[cfg(windows)]
fn linked_folder(_: &fs::DirEntry, kind: fs::FileType) -> bool {
    use std::os::windows::fs::FileTypeExt;
    kind.is_symlink_dir()
}

/// Whether a link is to a folder: elsewhere, its target's kind.
#[cfg(not(windows))]
fn linked_folder(item: &fs::DirEntry, _: fs::FileType) -> bool {
    fs::metadata(item.path()).is_ok_and(|m| m.is_dir())
}

/// Why `path` couldn't be opened for listing. Something there that isn't a folder is
/// [`ListError::NotFolder`], whatever the listing's own error said (Windows says a file's path
/// isn't found).
fn opening_error(path: &Path, e: &io::Error) -> ListError {
    if fs::metadata(path).is_ok_and(|m| !m.is_dir()) {
        return ListError::NotFolder;
    }
    error(e)
}

fn error(e: &io::Error) -> ListError {
    match e.kind() {
        ErrorKind::NotFound => ListError::NotFound,
        ErrorKind::NotADirectory => ListError::NotFolder,
        ErrorKind::PermissionDenied => ListError::Denied,
        _ => ListError::Failed(e.raw_os_error()),
    }
}

/// The rows' order: folders first, then by name ignoring case, then (for names that differ only
/// in case, which a case-sensitive folder can hold) by the name itself, so the order is total.
fn order(a: &Entry, b: &Entry) -> Ordering {
    b.folder
        .cmp(&a.folder)
        .then_with(|| cmp_ignoring_case(&a.name, &b.name))
        .then_with(|| a.name.cmp(&b.name))
}

/// `a` and `b` compared as their characters lowered (as the table's filter lowers them): byte by
/// byte while they're ASCII, so the common names cost no case mapping. Lowering, not raising,
/// puts `_` before letters, as File Explorer does.
fn cmp_ignoring_case(a: &str, b: &str) -> Ordering {
    let (bytes_a, bytes_b) = (a.as_bytes(), b.as_bytes());
    let mut at = 0;
    while let (Some(&byte_a), Some(&byte_b)) = (bytes_a.get(at), bytes_b.get(at)) {
        if byte_a != byte_b {
            if !(byte_a.is_ascii() && byte_b.is_ascii()) {
                break;
            }
            let (low_a, low_b) = (byte_a.to_ascii_lowercase(), byte_b.to_ascii_lowercase());
            if low_a != low_b {
                return low_a.cmp(&low_b);
            }
        }
        at += 1;
    }
    if at == bytes_a.len() || at == bytes_b.len() {
        return bytes_a.len().cmp(&bytes_b.len());
    }
    // Where they first differ outside ASCII: from that character on, lowered per character.
    // Everything before it is equal once lowered, and is the same bytes up to ASCII case, so the
    // character starts at the same place in both.
    while !a.is_char_boundary(at) {
        at -= 1;
    }
    lowered(a.get(at..).unwrap_or("")).cmp(lowered(b.get(at..).unwrap_or("")))
}

/// `s`'s characters, each lowered.
fn lowered(s: &str) -> impl Iterator<Item = char> + '_ {
    s.chars().flat_map(char::to_lowercase)
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
    /// A batch of this listing came that didn't end in an error: the folder is there.
    found: bool,
    /// The rows, in their order ([`order`]): the folders, then the files.
    entries: Vec<Entry>,
    /// Over the rows in their own order: the view is the rows, and its current row is the
    /// keyboard's. Made again as batches come, the current row kept on its entry.
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
            found: false,
            entries: Vec::new(),
            table: TableModel::new(0),
        }
    }

    /// What it chooses: a folder, or a file of some types.
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

    /// The shown folder's drive or share (`C:\`, `\\server\share\`), for the drive list; `None`
    /// before the first open. Allocates nothing.
    #[must_use]
    pub fn root(&self) -> Option<&Path> {
        self.folder.as_deref()?.ancestors().last()
    }

    /// The current listing's generation.
    #[must_use]
    pub fn generation(&self) -> Generation {
        self.generation
    }

    /// Where the current listing stands.
    #[must_use]
    pub fn listing(&self) -> Listing {
        self.listing
    }

    /// Opens `path`: a new generation, no rows, [`Listing::Loading`], the path field showing it
    /// ([`display_text`]'s form). Nothing touches the file system: the returned request is the
    /// lister's to run.
    ///
    /// # Errors
    /// [`Refused`] if `path` holds `..` or has no root; nothing changes, not even the field's
    /// text.
    pub fn open(&mut self, path: &Path) -> Result<ListRequest, Refused> {
        refusal(path)?;
        self.generation = self.generation.next();
        self.folder = Some(path.to_path_buf());
        self.path.clear();
        push_display(&path.to_string_lossy(), &mut self.path);
        self.listing = Listing::Loading;
        self.found = false;
        self.entries.clear();
        self.reset_table(0);
        Ok(ListRequest {
            generation: self.generation,
            path: path.to_path_buf(),
        })
    }

    /// Enter in the path field: opens what it says, its escapes turned back into the characters
    /// they show ([`real_text`]). Spaces around it, and the quotes File Explorer's "Copy as path"
    /// puts around a path, are left out.
    ///
    /// # Errors
    /// As [`open`](Self::open): a refused path stays in the field, for the user to fix.
    pub fn go(&mut self) -> Result<ListRequest, Refused> {
        let typed = self.path.trim();
        let typed = typed
            .strip_prefix('"')
            .and_then(|t| t.strip_suffix('"'))
            .unwrap_or(typed);
        let path = PathBuf::from(&*real_text(typed));
        self.open(&path)
    }

    /// Up one level (Up, Alt+Up, Backspace in the table): opens the shown folder's parent.
    /// `None` at a root (a drive, a share), or before the first open.
    pub fn up(&mut self) -> Option<ListRequest> {
        let parent = self.folder.as_deref()?.parent()?.to_path_buf();
        self.open(&parent).ok()
    }

    /// Enter on the row at `row` (source order): opens a folder, or chooses a file. `None` for
    /// no such row.
    pub fn activate(&mut self, row: u32) -> Option<Activated> {
        let entry = self.entry(row)?;
        let path = self.folder.as_deref()?.join(&*real_text(&entry.name));
        if entry.folder {
            self.open(&path).ok().map(Activated::Open)
        } else {
            Some(Activated::Chose(path))
        }
    }

    /// Takes a batch: its entries join the rows, in their places (those a folder picker or the
    /// file types leave out dropped), if it's the current generation's and the listing hasn't
    /// ended. Returns whether it was; a stale batch changes nothing. The current row stays on
    /// its entry. O(rows + batch × log batch).
    pub fn accept(&mut self, batch: ListBatch) -> bool {
        if batch.generation != self.generation || self.listing != Listing::Loading {
            return false;
        }
        let mut entries = batch.entries;
        let mode = &self.mode;
        entries.retain(|e| shows(mode, e));
        entries.sort_by(order);
        let current = self.table.current().map(|r| r as usize);
        let current = if self.entries.is_empty() {
            self.entries = entries;
            None
        } else if entries.is_empty() {
            current
        } else {
            let (merged, moved) = merge(mem::take(&mut self.entries), entries, current);
            self.entries = merged;
            moved
        };
        match batch.end {
            None => self.found = true,
            Some(Ok(())) => {
                self.found = true;
                self.listing = Listing::Listed;
            }
            Some(Err(e)) => self.listing = Listing::Failed(e),
        }
        self.reset_table(self.len());
        if let Some(at) = current.and_then(|c| u32::try_from(c).ok()) {
            self.table.set_current(at);
        }
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

    /// The rows' table, for the view's events (the current row moved, a row selected).
    pub fn table_mut(&mut self) -> &mut TableModel {
        &mut self.table
    }

    /// What Select chooses: choosing a folder, the shown folder once its listing found it;
    /// choosing a file, the current row's file. `None` if there's nothing to choose (Select is
    /// disabled: [`can_choose`](Self::can_choose)).
    #[must_use]
    pub fn choice(&self) -> Option<PathBuf> {
        match self.mode {
            Mode::Folder => self.found_folder().map(Path::to_path_buf),
            Mode::File(_) => {
                let file = self.current_file()?;
                Some(self.folder.as_deref()?.join(&*real_text(&file.name)))
            }
        }
    }

    /// Whether [`choice`](Self::choice) has something, without making its path: for a view that
    /// enables Select on every build.
    #[must_use]
    pub fn can_choose(&self) -> bool {
        match self.mode {
            Mode::Folder => self.found_folder().is_some(),
            Mode::File(_) => self.current_file().is_some(),
        }
    }

    /// The shown folder, once a batch of its listing said it's there: a folder that isn't, or
    /// hasn't answered yet, can't be chosen.
    fn found_folder(&self) -> Option<&Path> {
        self.folder.as_deref().filter(|_| self.found)
    }

    /// The current row, if it's a file.
    fn current_file(&self) -> Option<&Entry> {
        self.entry(self.table.current()?).filter(|e| !e.folder)
    }

    /// A table of `rows` rows in their order, nothing current. Its revision moves on, so a view
    /// that compares it sees the change.
    fn reset_table(&mut self, rows: u32) {
        let mut rev = self.table.rev;
        rev.bump();
        self.table = TableModel::new(rows);
        self.table.rev = rev;
    }
}

/// Why the picker won't list `path`, if it won't.
fn refusal(path: &Path) -> Result<(), Refused> {
    if path.components().any(|c| c == Component::ParentDir) {
        Err(Refused::Parent)
    } else if !path.is_absolute() {
        Err(Refused::Relative)
    } else {
        Ok(())
    }
}

/// Whether a picker in `mode` shows `entry`: folders always, files only choosing a file of an
/// allowed type.
fn shows(mode: &Mode, entry: &Entry) -> bool {
    match mode {
        Mode::Folder => entry.folder,
        Mode::File(types) => entry.folder || types.allows(&entry.name),
    }
}

/// `rows` and `batch`, both in [`order`], merged in it; and where row `current` went.
fn merge(
    rows: Vec<Entry>,
    batch: Vec<Entry>,
    current: Option<usize>,
) -> (Vec<Entry>, Option<usize>) {
    let mut out = Vec::with_capacity(rows.len() + batch.len());
    let mut moved = None;
    let mut rows = rows.into_iter().enumerate().peekable();
    let mut batch = batch.into_iter().peekable();
    loop {
        let from_rows = match (rows.peek(), batch.peek()) {
            (Some((_, r)), Some(b)) => order(r, b) != Ordering::Greater,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => break,
        };
        if from_rows {
            if let Some((i, r)) = rows.next() {
                if Some(i) == current {
                    moved = Some(out.len());
                }
                out.push(r);
            }
        } else if let Some(b) = batch.next() {
            out.push(b);
        }
    }
    (out, moved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(name: &str, folder: bool) -> Entry {
        Entry {
            name: name.into(),
            folder,
            modified: None,
        }
    }

    fn full(rest: &str) -> PathBuf {
        let root = if cfg!(windows) { r"C:\" } else { "/" };
        Path::new(root).join(rest)
    }

    #[test]
    fn names_compare_ignoring_case_ascii_or_not() {
        assert_eq!(cmp_ignoring_case("alpha", "Beta"), Ordering::Less);
        assert_eq!(cmp_ignoring_case("ALPHA", "alpha"), Ordering::Equal);
        assert_eq!(cmp_ignoring_case("abc", "ABCD"), Ordering::Less);
        assert_eq!(cmp_ignoring_case("_x", "A"), Ordering::Less, "lowered");
        assert_eq!(cmp_ignoring_case("Éclair", "écrou"), Ordering::Less);
        assert_eq!(cmp_ignoring_case("ÉCLAIR", "éclair"), Ordering::Equal);
        assert_eq!(cmp_ignoring_case("Zeta", "Ä"), Ordering::Less);
        // A difference after a shared non-ASCII character.
        assert_eq!(cmp_ignoring_case("Öb", "öA"), Ordering::Greater);
    }

    #[test]
    fn the_order_is_total_and_folders_come_first() {
        let mut v = [
            named("b", false),
            named("a", true),
            named("A", false),
            named("a", false),
            named("B", true),
        ];
        v.sort_by(order);
        let got: Vec<_> = v.iter().map(|e| (&*e.name, e.folder)).collect();
        assert_eq!(
            got,
            [
                ("a", true),
                ("B", true),
                ("A", false),
                ("a", false),
                ("b", false)
            ]
        );
    }

    #[test]
    fn a_merge_keeps_the_order_and_follows_the_current_row() {
        let rows = vec![named("a", true), named("c", true), named("x", false)];
        let batch = vec![named("b", true), named("d", true), named("y", false)];
        let (out, moved) = merge(rows, batch, Some(2));
        let names: Vec<_> = out.iter().map(|e| &*e.name).collect();
        assert_eq!(names, ["a", "b", "c", "d", "x", "y"]);
        assert_eq!(moved, Some(4), "x moved below the new folders");
    }

    #[test]
    fn the_current_row_stays_on_its_entry_as_batches_come() {
        let mut m = FolderModel::new(Mode::File(Types::new(&["*"])));
        let request = m.open(&full("")).expect("a full path");
        let batch = |entries| ListBatch {
            generation: request.generation,
            entries,
            end: None,
        };
        assert!(m.accept(batch(vec![named("m", true), named("z.txt", false)])));
        m.table_mut().set_current(1);
        let rev = m.table().rev;
        assert!(m.accept(batch(vec![named("a", true)])));
        assert_eq!(m.table().current(), Some(2), "z.txt, below the new folder");
        assert_ne!(m.table().rev, rev, "a view that compares sees the change");
        assert!(m.can_choose());
        assert_eq!(m.choice(), Some(full("z.txt")));
    }

    #[test]
    fn types_take_the_dialog_s_patterns() {
        let t = Types::new(&["*.WIM; *.esd;", ".swm", "iso"]);
        for yes in ["a.wim", "b.ESD", "c.swm", "d.iso", ".wim"] {
            assert!(t.allows(yes), "{yes}");
        }
        for no in ["wim", "a.wimx", "a.wim.bak", "a"] {
            assert!(!t.allows(no), "{no}");
        }
        assert!(Types::new(&[]).allows("anything"));
        assert!(Types::new(&["*.wim", "*.*"]).allows("notes.txt"));
        assert!(!Types::new(&["install*.wim"]).allows("install.wim"));
        // Lowered per character, as the names are.
        assert!(Types::new(&["*.ÄBC"]).allows("x.äBc"));
    }

    /// A3.8: a name that would disguise its extension shows its override as an escape (as a
    /// lister hands it), the file types judge it as the real name, and opening or choosing it, or
    /// going where the field shows, uses the real path.
    #[test]
    fn untrusted_names_show_escaped_and_open_as_themselves() {
        let listed = |name: &str, folder| named(&crate::display::display_text(name), folder);
        let mut m = FolderModel::new(Mode::File(Types::new(&["*.jpg"])));
        let request = m.open(&full("in")).expect("a full path");
        assert!(m.accept(ListBatch {
            generation: request.generation,
            entries: vec![
                listed("photo\u{202E}gpj.exe", false),
                listed("x\u{1b}[31m.jpg", false),
                listed("dir\u{2067}", true)
            ],
            end: Some(Ok(())),
        }));
        let names: Vec<_> = (0..m.len())
            .filter_map(|r| m.entry(r).map(|e| &*e.name))
            .collect();
        assert_eq!(
            names,
            ["dir<U+2067>", "x<U+001B>[31m.jpg"],
            "the .exe isn't a .jpg, whatever it shows"
        );
        m.table_mut().set_current(1);
        assert_eq!(m.choice(), Some(full("in").join("x\u{1b}[31m.jpg")));
        let Some(Activated::Open(opened)) = m.activate(0) else {
            panic!("the folder opens");
        };
        let real = full("in").join("dir\u{2067}");
        assert_eq!(opened.path, real);
        assert!(m.path().ends_with("dir<U+2067>"), "{}", m.path());
        assert!(!m.path().contains('\u{2067}'));
        let again = m.go().expect("the field's path");
        assert_eq!(again.path, real, "the field's escapes go back");
    }

    #[test]
    fn go_leaves_out_spaces_and_explorer_s_quotes() {
        let path = full("x y");
        let mut m = FolderModel::new(Mode::Folder);
        *m.path_mut() = format!("  \"{}\" ", path.display());
        let request = m.go().expect("a full path");
        assert_eq!(request.path, path);
        assert_eq!(m.path(), path.to_string_lossy());
    }

    #[test]
    fn a_found_folder_can_be_chosen_and_a_missing_one_cannot() {
        let path = full("x");
        let mut m = FolderModel::new(Mode::Folder);
        let request = m.open(&path).expect("a full path");
        assert!(!m.can_choose(), "not answered yet");
        assert!(m.accept(ListBatch {
            generation: request.generation,
            entries: Vec::new(),
            end: Some(Err(ListError::NotFound)),
        }));
        assert!(!m.can_choose());
        assert_eq!(m.choice(), None);
        let request = m.open(&path).expect("a full path");
        assert!(m.accept(ListBatch {
            generation: request.generation,
            entries: vec![named("a", true)],
            end: None,
        }));
        assert_eq!(m.choice(), Some(path.clone()), "found while it lists");
        assert_eq!(m.root(), path.parent());
    }

    /// `\\?\` paths and UNC paths are full paths: the field takes them, a pasted one too, and Up
    /// stops at their root (A3.5 (12), A3.8). T2 runs it.
    #[cfg(windows)]
    #[test]
    fn long_and_unc_paths_are_full_paths_and_up_stops_at_their_root() {
        for (typed, root) in [
            (r"\\?\C:\Windows\WinSxS", r"\\?\C:\"),
            (r"\\server\share\images", r"\\server\share\"),
            (r"\\?\UNC\server\share\images", r"\\?\UNC\server\share\"),
        ] {
            let mut m = FolderModel::new(Mode::Folder);
            *m.path_mut() = format!("\"{typed}\"");
            let request = m.go().expect(typed);
            assert_eq!(request.path, Path::new(typed));
            assert_eq!(m.root(), Some(Path::new(root)), "{typed}");
            assert!(m.up().is_some(), "{typed} has a parent");
            let mut ups = 1;
            while m.up().is_some() {
                ups += 1;
            }
            assert_eq!(
                m.folder(),
                Some(Path::new(root)),
                "{typed}: Up stops at its root"
            );
            assert!(ups <= 2, "{typed}: {ups} levels");
        }
        let mut m = FolderModel::new(Mode::Folder);
        for refused in [r"\\?\C:\Windows\..\Users", r"C:Windows", r"\Windows"] {
            *m.path_mut() = refused.to_owned();
            assert!(m.go().is_err(), "{refused}");
        }
    }
}
