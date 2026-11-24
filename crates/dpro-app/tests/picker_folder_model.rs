//! The in-app picker's folder model (TD-0009 A3.5 (12), A3.8, A3.9; slice 6e): a batch of an older
//! listing is dropped, folders come first, a file picker shows only the allowed types, `..` and
//! relative paths are refused before anything is listed, and a listing that never answers leaves
//! the model answering Up, Enter and new paths (A3.7 U19's exact T1 test, with the composite's
//! keys and Cancel in `dpro-ui`'s `composites_picker.rs`). [`FsLister`] lists a real folder in
//! batches of [`BATCH`], one level.

#![allow(missing_docs)]

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use dpro_app::picker::{
    Activated, BATCH, Entry, FolderModel, FsLister, Generation, ListBatch, ListError, ListRequest,
    Lister, Listing, Mode, Refused, Types,
};
use dpro_core::units::UnixMillis;

/// A folder's root for these tests: a full path on Windows and on Linux (T1).
fn root() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"C:\")
    } else {
        PathBuf::from("/")
    }
}

fn folder(name: &str) -> Entry {
    Entry {
        name: name.into(),
        folder: true,
        modified: Some(UnixMillis(1_700_000_000_000)),
    }
}

fn file(name: &str) -> Entry {
    Entry {
        name: name.into(),
        folder: false,
        modified: None,
    }
}

fn batch(request: &ListRequest, entries: Vec<Entry>, last: bool) -> ListBatch {
    ListBatch {
        generation: request.generation,
        entries,
        end: last.then_some(Ok(())),
    }
}

/// The rows' names, top to bottom in source order.
fn names(m: &FolderModel) -> Vec<String> {
    (0..m.len())
        .filter_map(|i| m.entry(i))
        .map(|e| e.name.to_string())
        .collect()
}

/// A lister whose listings never answer: a folder on a dead network share.
#[derive(Debug, Default)]
struct Never {
    started: Vec<ListRequest>,
}

impl Lister for Never {
    fn start(&mut self, request: ListRequest) {
        self.started.push(request);
    }

    fn poll(&mut self) -> Option<ListBatch> {
        None
    }
}

#[test]
fn each_open_is_a_new_generation_and_a_stale_batch_changes_nothing() {
    let mut m = FolderModel::new(Mode::Folder);
    assert_eq!(m.listing(), Listing::Idle);
    let first = m.open(&root().join("first")).expect("a full path");
    let second = m.open(&root().join("second")).expect("a full path");
    assert_ne!(first.generation, second.generation);
    assert_eq!(m.generation(), second.generation);
    assert_eq!(m.listing(), Listing::Loading);
    // The first folder's batch comes late: the second one's rows aren't painted over.
    assert!(!m.accept(batch(&first, vec![folder("stale")], true)));
    assert!(m.is_empty());
    assert_eq!(m.listing(), Listing::Loading);
    assert!(m.accept(batch(&second, vec![folder("fresh")], true)));
    assert_eq!(names(&m), ["fresh"]);
    assert_eq!(m.listing(), Listing::Listed);
    // Once listed, nothing more joins, even of its own generation.
    assert!(!m.accept(batch(&second, vec![folder("after")], true)));
    assert_eq!(names(&m), ["fresh"]);
}

#[test]
fn generations_wrap_and_stay_distinct_from_their_neighbours() {
    let g = Generation::default();
    assert_ne!(g.next(), g);
    assert_ne!(g.next().next(), g.next());
}

#[test]
fn batches_join_the_rows_until_the_last() {
    let mut m = FolderModel::new(Mode::Folder);
    let request = m.open(&root()).expect("a full path");
    assert!(m.accept(batch(&request, vec![folder("a"), folder("b")], false)));
    assert_eq!(m.listing(), Listing::Loading);
    assert_eq!(m.len(), 2);
    assert!(m.accept(batch(&request, vec![folder("c")], true)));
    assert_eq!(m.listing(), Listing::Listed);
    assert_eq!(m.len(), 3);
    assert_eq!(m.table().rows(), 3, "the table has every row");
}

#[test]
fn folders_come_first_each_in_name_order_ignoring_case() {
    let types = Types::new(&["*.*"]);
    let mut m = FolderModel::new(Mode::File(types));
    let request = m.open(&root()).expect("a full path");
    assert!(m.accept(batch(
        &request,
        vec![file("b.txt"), folder("Zeta"), file("A.txt")],
        false
    )));
    // A later batch's folder still goes above every file, and in its place among the folders.
    assert!(m.accept(batch(
        &request,
        vec![folder("alpha"), file("c.txt"), folder("Mid")],
        true
    )));
    assert_eq!(
        names(&m),
        ["alpha", "Mid", "Zeta", "A.txt", "b.txt", "c.txt"]
    );
}

#[test]
fn a_file_picker_shows_folders_and_the_allowed_types_only() {
    let types = Types::new(&["*.wim;*.esd", "*.swm"]);
    assert!(types.allows("install.wim"));
    assert!(types.allows("INSTALL.ESD"), "ignoring case");
    assert!(types.allows("split.swm"));
    assert!(!types.allows("notes.txt"));
    assert!(!types.allows("wim"), "an extension, not a name");
    assert!(!types.allows("archive.wim.bak"));
    let mut m = FolderModel::new(Mode::File(types));
    let request = m.open(&root()).expect("a full path");
    assert!(m.accept(batch(
        &request,
        vec![
            file("notes.txt"),
            file("install.wim"),
            folder("sources"),
            file("boot.WIM"),
            file("readme"),
        ],
        true
    )));
    assert_eq!(names(&m), ["sources", "boot.WIM", "install.wim"]);
}

#[test]
fn every_file_is_allowed_by_a_star() {
    for all in ["*", "*.*"] {
        let types = Types::new(&[all]);
        assert!(types.allows("notes.txt"), "{all}");
        assert!(types.allows("readme"), "{all}");
    }
}

#[test]
fn a_folder_picker_lists_no_files() {
    let mut m = FolderModel::new(Mode::Folder);
    let request = m.open(&root()).expect("a full path");
    assert!(m.accept(batch(
        &request,
        vec![file("install.wim"), folder("sources"), file("notes.txt")],
        true
    )));
    assert_eq!(names(&m), ["sources"]);
}

#[test]
fn dot_dot_and_relative_paths_are_refused_before_anything_is_listed() {
    let mut m = FolderModel::new(Mode::Folder);
    let shown = m.open(&root().join("shown")).expect("a full path");
    for (path, why) in [
        (root().join("a").join("..").join("b"), Refused::Parent),
        (root().join(".."), Refused::Parent),
        (PathBuf::from("relative"), Refused::Relative),
        (PathBuf::new(), Refused::Relative),
    ] {
        assert_eq!(m.open(&path), Err(why), "{path:?}");
        // Nothing changed: the shown folder's listing goes on.
        assert_eq!(m.generation(), shown.generation, "{path:?}");
        assert_eq!(m.listing(), Listing::Loading);
        assert_eq!(m.folder(), Some(root().join("shown").as_path()));
    }
    // Typed into the field, the same: the text stays for the user to fix.
    *m.path_mut() = root().join("x").join("..").to_string_lossy().into_owned();
    assert_eq!(m.go(), Err(Refused::Parent));
    assert!(m.path().ends_with(".."));
    assert_eq!(m.generation(), shown.generation);
}

#[test]
fn up_opens_the_parent_and_stops_at_a_root() {
    let mut m = FolderModel::new(Mode::Folder);
    assert_eq!(m.up(), None, "nothing opened");
    let deep = root().join("a").join("b");
    let first = m.open(&deep).expect("a full path");
    let up = m.up().expect("b's parent");
    assert_eq!(up.path, root().join("a"));
    assert_ne!(up.generation, first.generation);
    assert_eq!(m.folder(), Some(root().join("a").as_path()));
    assert_eq!(m.path(), root().join("a").to_string_lossy());
    let top = m.up().expect("a's parent");
    assert_eq!(top.path, root());
    assert_eq!(m.up(), None, "a root has no parent");
    assert_eq!(m.generation(), top.generation);
}

#[test]
fn enter_on_a_folder_opens_it_and_on_a_file_chooses_it() {
    let mut m = FolderModel::new(Mode::File(Types::new(&["*.wim"])));
    let request = m.open(&root().join("images")).expect("a full path");
    assert!(m.accept(batch(
        &request,
        vec![file("install.wim"), folder("sources")],
        true
    )));
    // Folders first: row 0 is the folder.
    let Some(Activated::Open(open)) = m.activate(0) else {
        panic!("a folder's row opens it");
    };
    assert_eq!(open.path, root().join("images").join("sources"));
    assert_eq!(m.listing(), Listing::Loading);
    assert!(m.is_empty());
    assert!(m.accept(batch(&open, vec![file("boot.wim")], true)));
    assert_eq!(
        m.activate(0),
        Some(Activated::Chose(
            root().join("images").join("sources").join("boot.wim")
        ))
    );
    assert_eq!(m.activate(1), None, "past the rows");
}

#[test]
fn select_chooses_the_folder_shown_or_the_current_file() {
    let mut m = FolderModel::new(Mode::Folder);
    assert_eq!(m.choice(), None, "nothing opened");
    let request = m.open(&root().join("target")).expect("a full path");
    assert!(m.accept(batch(&request, vec![folder("sub")], true)));
    assert_eq!(m.choice(), Some(root().join("target")));

    let mut f = FolderModel::new(Mode::File(Types::new(&["*.wim"])));
    let request = f.open(&root().join("images")).expect("a full path");
    assert!(f.accept(batch(
        &request,
        vec![folder("sub"), file("install.wim")],
        true
    )));
    assert_eq!(f.choice(), None, "no file is current");
    f.table_mut().set_current(0);
    assert_eq!(f.choice(), None, "a folder isn't a file");
    f.table_mut().set_current(1);
    assert_eq!(f.choice(), Some(root().join("images").join("install.wim")));
}

#[test]
fn a_listing_ends_in_its_error() {
    for error in [
        ListError::NotFound,
        ListError::NotFolder,
        ListError::Denied,
        ListError::Failed(Some(5)),
    ] {
        let mut m = FolderModel::new(Mode::Folder);
        let request = m.open(&root().join("x")).expect("a full path");
        assert!(m.accept(ListBatch {
            generation: request.generation,
            entries: Vec::new(),
            end: Some(Err(error)),
        }));
        assert_eq!(m.listing(), Listing::Failed(error));
        // The field keeps what was typed, for the user to fix.
        assert_eq!(m.path(), root().join("x").to_string_lossy());
    }
}

/// A3.8's hanging listing: the model never waits, so Up, Enter in the field and a new folder
/// all answer at once, each a new generation the lister is asked for; what the hung listing
/// sends at last is dropped.
#[test]
fn a_listing_that_never_answers_leaves_the_model_answering() {
    let mut lister = Never::default();
    let mut m = FolderModel::new(Mode::Folder);
    let hung = m
        .open(&root().join("share").join("dead"))
        .expect("a full path");
    lister.start(hung.clone());
    assert_eq!(lister.poll(), None);
    assert_eq!(m.listing(), Listing::Loading);
    // Up, while it hangs.
    let up = m.up().expect("the parent");
    lister.start(up.clone());
    assert_eq!(up.path, root().join("share"));
    // A path typed into the field, while that hangs too.
    *m.path_mut() = root().join("elsewhere").to_string_lossy().into_owned();
    let typed = m.go().expect("a full path");
    lister.start(typed.clone());
    assert_eq!(
        lister
            .started
            .iter()
            .map(|r| r.generation)
            .collect::<Vec<_>>(),
        [hung.generation, up.generation, typed.generation]
    );
    // The hung listing answers at last: too late.
    assert!(!m.accept(batch(&hung, vec![folder("late")], true)));
    assert!(m.accept(batch(&typed, vec![folder("here")], true)));
    assert_eq!(names(&m), ["here"]);
}

/// The real lister on a folder of `BATCH * 2 + 21` subfolders, each with a file inside: batches
/// of at most [`BATCH`], one level, and a wake per batch.
#[test]
fn the_file_system_lister_sends_batches_of_one_level() {
    let dir = std::env::temp_dir().join(format!("dpro-picker-test-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    let count = BATCH * 2 + 21;
    for i in 0..count {
        let sub = dir.join(format!("folder {i:05}"));
        std::fs::create_dir_all(&sub).expect("a test folder");
        std::fs::write(sub.join("inner.txt"), b"x").expect("a test file");
    }
    std::fs::write(dir.join("top.txt"), b"x").expect("a test file");
    let wakes = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let woken = std::sync::Arc::clone(&wakes);
    let mut lister = FsLister::new(Box::new(move || {
        woken.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }));
    let mut m = FolderModel::new(Mode::File(Types::new(&["*.txt"])));
    let request = m.open(&dir).expect("a full path");
    lister.start(request);
    let (mut batches, start) = (0, Instant::now());
    while m.listing() == Listing::Loading && start.elapsed() < Duration::from_secs(30) {
        match lister.poll() {
            Some(b) => {
                assert!(b.entries.len() <= BATCH, "{}", b.entries.len());
                assert!(m.accept(b));
                batches += 1;
            }
            None => std::thread::sleep(Duration::from_millis(1)),
        }
    }
    assert_eq!(m.listing(), Listing::Listed);
    assert!(batches >= 3, "{batches} batches");
    assert!(wakes.load(std::sync::atomic::Ordering::SeqCst) >= 1);
    // One level: the subfolders and top.txt, none of the inner files.
    assert_eq!(m.len(), u32::try_from(count + 1).unwrap_or(u32::MAX));
    assert!(names(&m).iter().all(|n| n != "inner.txt"));
    assert_eq!(names(&m).last().map(String::as_str), Some("top.txt"));
    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn the_file_system_lister_says_not_found_and_not_folder() {
    let dir = std::env::temp_dir().join(format!("dpro-picker-errors-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(&dir).expect("a test folder");
    std::fs::write(dir.join("a file"), b"x").expect("a test file");
    for (path, error) in [
        (dir.join("missing"), ListError::NotFound),
        (dir.join("a file"), ListError::NotFolder),
    ] {
        let mut lister = FsLister::new(Box::new(|| {}));
        let mut m = FolderModel::new(Mode::Folder);
        lister.start(m.open(&path).expect("a full path"));
        let start = Instant::now();
        while m.listing() == Listing::Loading && start.elapsed() < Duration::from_secs(10) {
            match lister.poll() {
                Some(b) => drop(m.accept(b)),
                None => std::thread::sleep(Duration::from_millis(1)),
            }
        }
        assert_eq!(m.listing(), Listing::Failed(error), "{}", path.display());
    }
    drop(std::fs::remove_dir_all(&dir));
}

/// `Path` is only used for its type in signatures.
#[test]
fn a_folder_model_starts_empty() {
    let m = FolderModel::new(Mode::Folder);
    assert_eq!(m.folder(), None::<&Path>);
    assert_eq!(m.path(), "");
    assert!(m.is_empty());
    assert_eq!(m.choice(), None);
    assert_eq!(m.mode(), &Mode::Folder);
}
