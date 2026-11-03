use std::{borrow::Cow, collections::BTreeMap};

use proptest::prelude::*;

use super::{
    Regf, RegfError,
    testkit::{List, TestKey, build, seal, software_like},
};

const REG_SZ: u32 = 1;
const REG_BINARY: u32 = 3;
const REG_DWORD: u32 = 4;

fn utf16(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn hive(root: &TestKey) -> Regf {
    Regf::parse(build(root)).unwrap()
}

fn sample() -> TestKey {
    TestKey::new("ROOT")
        .value("", REG_SZ, &utf16("default\0"))
        .subkey(
            TestKey::new("Select")
                .value("Current", REG_DWORD, &1_u32.to_le_bytes())
                .value("Binary", REG_BINARY, &[9; 300]),
        )
        .subkey(
            TestKey::new("Microsoft").subkey(TestKey::new("Windows").value(
                "Build",
                REG_SZ,
                &utf16("26100\0"),
            )),
        )
}

#[test]
fn walks_paths_and_reads_values() {
    let hive = hive(&sample());
    let key = hive.key("Microsoft\\Windows").unwrap().unwrap();
    assert_eq!(key.name().unwrap(), "Windows");
    assert_eq!(key.last_written().unwrap(), 0x01DC_0000_0000_0000);
    let value = key.value("Build").unwrap().unwrap();
    assert_eq!(value.kind().unwrap(), REG_SZ);
    assert_eq!(value.data().unwrap().as_ref(), utf16("26100\0"));
    let select = hive.key("Select").unwrap().unwrap();
    let current = select.value("Current").unwrap().unwrap();
    assert_eq!(current.data().unwrap().as_ref(), 1_u32.to_le_bytes());
    assert!(matches!(current.data().unwrap(), Cow::Borrowed(_)));
    let binary = select.value("Binary").unwrap().unwrap();
    assert_eq!(binary.data().unwrap().as_ref(), [9; 300]);
}

#[test]
fn lookups_ignore_case_and_extra_separators() {
    let hive = hive(&sample());
    let key = hive.key("\\MICROSOFT\\\\windows\\").unwrap().unwrap();
    assert_eq!(key.name().unwrap(), "Windows");
    assert!(key.value("bUILD").unwrap().is_some());
}

#[test]
fn the_empty_path_is_the_root_and_the_empty_name_is_the_default_value() {
    let hive = hive(&sample());
    let root = hive.key("").unwrap().unwrap();
    assert_eq!(root.name().unwrap(), "ROOT");
    let default = root.value("").unwrap().unwrap();
    assert_eq!(default.name().unwrap(), "");
    assert_eq!(default.data().unwrap().as_ref(), utf16("default\0"));
}

#[test]
fn missing_keys_and_values_are_none() {
    let hive = hive(&sample());
    assert!(hive.key("Microsoft\\Nope").unwrap().is_none());
    assert!(hive.key("Select\\Current").unwrap().is_none());
    let select = hive.key("Select").unwrap().unwrap();
    assert!(select.value("Nope").unwrap().is_none());
    let leaf = hive.key("Microsoft\\Windows").unwrap().unwrap();
    assert!(leaf.subkey("Nope").unwrap().is_none());
    assert!(leaf.subkeys().unwrap().is_empty());
}

#[test]
fn names_compare_like_the_registry() {
    let root = TestKey::new("ROOT")
        .subkey(TestKey::new("Größe").value("Wert é", REG_DWORD, &[1, 0, 0, 0]))
        .subkey(TestKey::new("鍵").value("値", REG_DWORD, &[2, 0, 0, 0]))
        .subkey(TestKey::new("Ω"));
    let hive = hive(&root);
    // Latin-1 stored: uppercase folds é, and ß doesn't expand to SS.
    let key = hive.key("GRÖßE").unwrap().unwrap();
    assert_eq!(key.name().unwrap(), "Größe");
    assert!(key.value("WERT É").unwrap().is_some());
    assert!(hive.key("GROESSE").unwrap().is_none());
    assert!(hive.key("GRÖSSE").unwrap().is_none());
    // UTF-16 stored.
    let key = hive.key("鍵").unwrap().unwrap();
    assert_eq!(key.value("値").unwrap().unwrap().name().unwrap(), "値");
    assert!(hive.key("ω").unwrap().is_some());
}

#[test]
fn every_subkey_list_form_is_read() {
    for list in [List::Li, List::Lf, List::Lh, List::Ri(3)] {
        let mut root = TestKey::new("ROOT").list(list);
        for i in 0..10 {
            root = root.subkey(TestKey::new(&format!("K{i}")));
        }
        let hive = hive(&root);
        let names: Vec<String> = hive
            .root()
            .unwrap()
            .subkeys()
            .unwrap()
            .iter()
            .map(|k| k.name().unwrap())
            .collect();
        assert_eq!(names.len(), 10, "{list:?}");
        assert!(hive.key("K9").unwrap().is_some(), "{list:?}");
    }
}

#[test]
fn data_sizes_on_every_storage_path() {
    let pattern =
        |n: usize| -> Vec<u8> { (0..n).map(|i| u8::try_from(i % 251).unwrap()).collect() };
    for n in [0, 1, 3, 4, 5, 8, 16344, 16345, 32688, 32689, 100_000] {
        let hive = hive(&TestKey::new("ROOT").value("v", REG_BINARY, &pattern(n)));
        let value = hive.root().unwrap().value("v").unwrap().unwrap();
        assert_eq!(value.data().unwrap().as_ref(), pattern(n), "{n} bytes");
    }
}

#[test]
fn values_lists_all_in_hive_order() {
    let hive = hive(
        &TestKey::new("ROOT")
            .value("a", REG_DWORD, &[1, 0, 0, 0])
            .value("b", REG_DWORD, &[2, 0, 0, 0]),
    );
    let values = hive.root().unwrap().values().unwrap();
    let names: Vec<String> = values.iter().map(|v| v.name().unwrap()).collect();
    assert_eq!(names, ["a", "b"]);
}

#[test]
fn opens_a_file_and_leaves_it_untouched() {
    let dir = std::env::temp_dir().join(format!("dpro-regf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("SOFTWARE");
    let image = build(&sample());
    std::fs::write(&path, &image).unwrap();
    let hive = Regf::open(&path).unwrap();
    assert!(hive.key("Select").unwrap().is_some());
    drop(hive);
    assert_eq!(std::fs::read(&path).unwrap(), image);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_missing_file_names_the_path() {
    let path = std::env::temp_dir()
        .join("dpro-regf-missing")
        .join("SYSTEM");
    let error = Regf::open(&path).unwrap_err();
    assert!(matches!(error, RegfError::Io { .. }));
    assert!(error.to_string().contains("SYSTEM"), "{error}");
}

#[test]
fn differing_sequence_numbers_mean_dirty() {
    let mut image = build(&sample());
    assert!(!Regf::parse(image.clone()).unwrap().is_dirty());
    image[0x04] = 2;
    seal(&mut image);
    let dirty = Regf::parse(image).unwrap();
    assert!(dirty.is_dirty());
    assert!(dirty.key("Select").unwrap().is_some());
}

#[test]
fn the_base_block_is_checked() {
    let image = build(&sample());
    let edit = |at: usize, byte: u8| {
        let mut bad = image.clone();
        bad[at] = byte;
        seal(&mut bad);
        Regf::parse(bad).unwrap_err()
    };
    assert!(matches!(
        Regf::parse(Vec::new()),
        Err(RegfError::NotAHive("bad signature"))
    ));
    assert!(matches!(
        Regf::parse(b"regf".to_vec()),
        Err(RegfError::NotAHive(_))
    ));
    assert!(matches!(
        edit(0, b'x'),
        RegfError::NotAHive("bad signature")
    ));
    let mut bad_sum = image.clone();
    bad_sum[0x40] ^= 1;
    assert!(matches!(
        Regf::parse(bad_sum),
        Err(RegfError::NotAHive("bad base block checksum"))
    ));
    assert!(matches!(
        edit(0x14, 2),
        RegfError::Unsupported { major: 2, minor: 5 }
    ));
    assert!(matches!(
        edit(0x18, 2),
        RegfError::Unsupported { major: 1, minor: 2 }
    ));
    assert!(matches!(edit(0x1C, 1), RegfError::NotAHive(_)));
    assert!(matches!(edit(0x20, 0), RegfError::NotAHive(_)));
    assert!(matches!(edit(0x2A, 0x7F), RegfError::Corrupt { .. }));
    let mut short = image.clone();
    short.truncate(0x1800);
    assert!(matches!(Regf::parse(short), Err(RegfError::Corrupt { .. })));
}

/// Offset of the first cell whose payload starts with `signature`.
fn find(image: &[u8], signature: [u8; 2]) -> usize {
    image
        .windows(2)
        .skip(0x1000)
        .position(|w| w == signature)
        .map(|at| at + 0x1000)
        .unwrap()
}

fn root_nk(image: &[u8]) -> usize {
    let root = u32::from_le_bytes(image[0x24..0x28].try_into().unwrap());
    0x1000 + root as usize + 4
}

fn set_u32(image: &mut [u8], at: usize, value: u32) {
    image[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

#[test]
fn a_free_cell_or_an_offset_past_the_end_is_an_error() {
    let image = build(&sample());
    for offset in [u32::MAX, u32::MAX - 3, 0x7FFF_0000] {
        let mut bad = image.clone();
        set_u32(&mut bad, 0x24, offset);
        seal(&mut bad);
        let hive = Regf::parse(bad).unwrap();
        assert!(matches!(hive.root(), Err(RegfError::Corrupt { .. })));
    }
    let mut bad = image.clone();
    let root = root_nk(&bad);
    let size_at = root - 4;
    let size = i32::from_le_bytes(bad[size_at..size_at + 4].try_into().unwrap());
    bad[size_at..size_at + 4].copy_from_slice(&(-size).to_le_bytes());
    assert!(matches!(
        Regf::parse(bad).unwrap().root(),
        Err(RegfError::Corrupt {
            what: "reference to a free cell",
            ..
        })
    ));
}

#[test]
fn a_cell_larger_than_the_hive_is_an_error() {
    let mut image = build(&sample());
    let root = root_nk(&image);
    image[root - 4..root].copy_from_slice(&i32::MIN.to_le_bytes());
    let hive = Regf::parse(image).unwrap();
    assert!(matches!(hive.root(), Err(RegfError::Corrupt { .. })));
}

#[test]
fn a_subkey_count_beyond_the_file_is_refused_before_walking() {
    let mut image = build(&sample());
    let root = root_nk(&image);
    set_u32(&mut image, root + 0x14, u32::MAX);
    let hive = Regf::parse(image).unwrap();
    assert!(matches!(
        hive.root().unwrap().subkeys(),
        Err(RegfError::Corrupt {
            what: "impossible subkey count",
            ..
        })
    ));
}

#[test]
fn a_list_longer_than_the_declared_count_is_an_error() {
    let mut image = build(&sample());
    let root = root_nk(&image);
    set_u32(&mut image, root + 0x14, 1);
    let hive = Regf::parse(image).unwrap();
    assert!(matches!(
        hive.root().unwrap().subkeys(),
        Err(RegfError::Corrupt {
            what: "more subkeys than the key declares",
            ..
        })
    ));
}

#[test]
fn an_ri_list_may_not_nest() {
    let root = TestKey::new("ROOT")
        .list(List::Ri(1))
        .subkey(TestKey::new("A"));
    let mut image = build(&root);
    // Point the ri at itself.
    let ri = find(&image, *b"ri");
    let own = u32::try_from(ri - 4 - 0x1000).unwrap();
    set_u32(&mut image, ri + 4, own);
    let hive = Regf::parse(image).unwrap();
    assert!(matches!(
        hive.root().unwrap().subkeys(),
        Err(RegfError::Corrupt {
            what: "expected a subkey list",
            ..
        })
    ));
}

#[test]
fn a_key_that_lists_itself_stays_bounded() {
    let mut image = build(&TestKey::new("ROOT").subkey(TestKey::new("A")));
    let own = u32::from_le_bytes(image[0x24..0x28].try_into().unwrap());
    let lh = find(&image, *b"lh");
    set_u32(&mut image, lh + 4, own);
    let hive = Regf::parse(image).unwrap();
    // Every step down is the same key: paths end, and subkeys() has one entry per list slot.
    let key = hive.key("ROOT\\ROOT\\ROOT").unwrap().unwrap();
    assert_eq!(key.name().unwrap(), "ROOT");
    assert_eq!(hive.root().unwrap().subkeys().unwrap().len(), 1);
}

#[test]
fn value_corruption_is_an_error() {
    let image = build(&TestKey::new("ROOT").value("v", REG_BINARY, &[7; 64]));
    let vk = find(&image, *b"vk");
    let data_of = |image: Vec<u8>| {
        let hive = Regf::parse(image).unwrap();
        let root = hive.root().unwrap();
        let value = root.value("v").unwrap().unwrap();
        value.data().map(|d| d.len())
    };
    let mut long = image.clone();
    set_u32(&mut long, vk + 4, 0x7FFF_FFFF);
    assert!(matches!(data_of(long), Err(RegfError::Corrupt { .. })));
    let mut inline = image.clone();
    set_u32(&mut inline, vk + 4, 0x8000_0005);
    assert!(matches!(data_of(inline), Err(RegfError::Corrupt { .. })));
    let mut wild = image.clone();
    set_u32(&mut wild, vk + 8, u32::MAX);
    assert!(matches!(data_of(wild), Err(RegfError::Corrupt { .. })));
}

#[test]
fn big_data_corruption_is_an_error() {
    let image = build(&TestKey::new("ROOT").value("v", REG_BINARY, &vec![7; 40_000]));
    let db = find(&image, *b"db");
    let vk = find(&image, *b"vk");
    let data_of = |image: Vec<u8>| {
        let hive = Regf::parse(image).unwrap();
        let root = hive.root().unwrap();
        let value = root.value("v").unwrap().unwrap();
        value.data().map(|d| d.len())
    };
    assert_eq!(data_of(image.clone()).unwrap(), 40_000);
    // More data declared than the segments hold.
    let mut short = image.clone();
    set_u32(&mut short, vk + 4, 40_100);
    assert!(matches!(data_of(short), Err(RegfError::Corrupt { .. })));
    // Fewer segments than the data needs.
    let mut cut = image.clone();
    cut[db + 2] = 1;
    assert!(matches!(data_of(cut), Err(RegfError::Corrupt { .. })));
    // A size no file could hold is refused before allocating.
    let mut huge = image.clone();
    set_u32(&mut huge, vk + 4, 0x7FFF_FFFF);
    assert!(matches!(
        data_of(huge),
        Err(RegfError::Corrupt {
            what: "impossible data size",
            ..
        })
    ));
    // Far more segments than the list cell holds.
    let mut many = image;
    many[db + 2..db + 4].copy_from_slice(&u16::MAX.to_le_bytes());
    assert!(matches!(data_of(many), Err(RegfError::Corrupt { .. })));
}

/// Reads everything reachable, as a scan would. A corrupt image may fail, but never panics.
fn walk(hive: &Regf) -> Result<usize, RegfError> {
    fn visit(key: super::Key<'_>, depth: usize) -> Result<usize, RegfError> {
        let mut seen = key.name()?.len();
        key.last_written()?;
        for value in key.values()? {
            seen += value.name()?.len() + value.data()?.len();
            value.kind()?;
        }
        if depth < 8 {
            for sub in key.subkeys()? {
                seen += visit(sub, depth + 1)?;
            }
        }
        Ok(seen)
    }
    visit(hive.root()?, 0)
}

type Model = BTreeMap<String, Vec<u8>>;

fn arb_name() -> impl Strategy<Value = String> {
    prop_oneof!["[A-Za-z0-9 _.-]{1,12}", "[a-zäöüßéΩ鍵値]{1,6}"]
}

prop_compose! {
    fn arb_leaf()(
        name in arb_name(),
        values in prop::collection::btree_map(
            arb_name(),
            prop::collection::vec(any::<u8>(), 0..64),
            0..5,
        ),
    ) -> (String, Model) {
        (name, values)
    }
}

fn arb_tree() -> impl Strategy<Value = TestKey> {
    let form = prop_oneof![
        Just(List::Li),
        Just(List::Lf),
        Just(List::Lh),
        (1_usize..4).prop_map(List::Ri)
    ];
    (prop::collection::vec(arb_leaf(), 0..12), form).prop_map(|(leaves, form)| {
        let mut unique = BTreeMap::new();
        for (name, values) in leaves {
            unique.entry(name.to_uppercase()).or_insert((name, values));
        }
        let mut root = TestKey::new("ROOT").list(form);
        for (name, values) in unique.into_values() {
            let mut key = TestKey::new(&name);
            let mut names = BTreeMap::new();
            for (value, data) in values {
                names.entry(value.to_uppercase()).or_insert((value, data));
            }
            for (value, data) in names.into_values() {
                key = key.value(&value, REG_BINARY, &data);
            }
            root = root.subkey(key);
        }
        root
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// What was built reads back, key by key and value by value.
    #[test]
    fn reads_back_what_was_built(tree in arb_tree()) {
        let hive = Regf::parse(build(&tree)).unwrap();
        let root = hive.root().unwrap();
        prop_assert_eq!(root.subkeys().unwrap().len(), tree.subkeys.len());
        for sub in &tree.subkeys {
            let key = root.subkey(&sub.name).unwrap().unwrap();
            prop_assert_eq!(key.name().unwrap(), sub.name.clone());
            prop_assert_eq!(key.values().unwrap().len(), sub.values.len());
            for (name, kind, data) in &sub.values {
                let value = key.value(name).unwrap().unwrap();
                prop_assert_eq!(value.kind().unwrap(), *kind);
                let read = value.data().unwrap();
                prop_assert_eq!(read.as_ref(), data.as_slice());
            }
        }
    }

    /// Any damage to the cells gives an answer or an error. In release a panic aborts the tool.
    #[test]
    fn damaged_hives_never_panic(
        tree in arb_tree(),
        edits in prop::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..32),
    ) {
        let mut image = build(&tree);
        let cells = image.len() - 0x1000;
        for (at, byte) in edits {
            image[0x1000 + at.index(cells)] = byte;
        }
        let hive = Regf::parse(image).unwrap();
        let _ = walk(&hive);
        let _ = hive.key("a\\b\\c");
    }

    /// The header too, with the checksum repaired so parsing goes on.
    #[test]
    fn damaged_headers_never_panic(
        tree in arb_tree(),
        edits in prop::collection::vec((0x0_usize..0x30, any::<u8>()), 1..8),
    ) {
        let mut image = build(&tree);
        for (at, byte) in edits {
            image[at] = byte;
        }
        seal(&mut image);
        if let Ok(hive) = Regf::parse(image) {
            let _ = walk(&hive);
        }
    }

    #[test]
    fn arbitrary_bytes_never_panic(mut bytes in prop::collection::vec(any::<u8>(), 0..0x2400)) {
        if bytes.len() >= 4 {
            bytes[..4].copy_from_slice(b"regf");
        }
        if let Ok(hive) = Regf::parse(bytes) {
            let _ = walk(&hive);
        }
    }
}

#[test]
fn a_machine_sized_hive_reads_like_a_small_one() {
    let image = software_like(1 << 20);
    assert!(image.len() > 1 << 19, "{} bytes", image.len());
    let hive = Regf::parse(image).unwrap();
    let key = hive
        .key(r"Microsoft\Windows NT\CurrentVersion")
        .unwrap()
        .unwrap();
    assert!(key.value("CurrentBuild").unwrap().is_some());
    assert!(hive.key(r"VENDOR0199\KEY000000").unwrap().is_some());
    assert_eq!(hive.root().unwrap().subkeys().unwrap().len(), 201);
}

#[test]
fn deep_paths_resolve() {
    let mut key = TestKey::new("leaf");
    for _ in 0..64 {
        key = TestKey::new("n").subkey(key);
    }
    let hive = hive(&TestKey::new("ROOT").subkey(key));
    let path = vec!["n"; 64].join("\\");
    assert!(hive.key(&format!("{path}\\leaf")).unwrap().is_some());
}
