//! The Windows side: the six enumeration methods, corpus creation, the stale-plan experiment and
//! the volume probe. Every `unsafe` block is one Win32 call on memory this module owns.

use std::{
    collections::HashSet,
    ffi::c_void,
    fmt::Write as _,
    mem::{offset_of, size_of},
    os::windows::ffi::OsStrExt,
    path::Path,
    ptr,
    sync::{
        Condvar, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use windows_sys::Win32::{
    Foundation::{
        CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_HANDLE_EOF, ERROR_NO_MORE_FILES,
        ERROR_NOT_ALL_ASSIGNED, GENERIC_READ, GENERIC_WRITE, GetLastError, HANDLE,
        INVALID_HANDLE_VALUE, LUID,
    },
    Security::{
        AdjustTokenPrivileges, LUID_AND_ATTRIBUTES, LookupPrivilegeValueW, SE_PRIVILEGE_ENABLED,
        TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES,
    },
    Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, CREATE_NEW, CreateDirectoryW, CreateFileW, CreateHardLinkW,
        FILE_APPEND_DATA, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_BEGIN, FILE_END_OF_FILE_INFO,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_ID_EXTD_DIR_INFO, FILE_LIST_DIRECTORY,
        FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        FIND_FIRST_EX_LARGE_FETCH, FileEndOfFileInfo, FileIdExtdDirectoryInfo, FindClose,
        FindExInfoBasic, FindExSearchNameMatch, FindFirstFileExW, FindNextFileW,
        GetFileAttributesExW, GetFileExInfoStandard, GetFileInformationByHandle,
        GetFileInformationByHandleEx, GetVolumePathNameW, OPEN_EXISTING, ReadFile,
        SetFileInformationByHandle, SetFilePointerEx, WIN32_FILE_ATTRIBUTE_DATA, WIN32_FIND_DATAW,
        WriteFile,
    },
    System::{
        IO::DeviceIoControl,
        Ioctl::{
            FSCTL_DISMOUNT_VOLUME, FSCTL_ENUM_USN_DATA, FSCTL_GET_NTFS_VOLUME_DATA,
            FSCTL_QUERY_USN_JOURNAL, FSCTL_READ_USN_JOURNAL, MFT_ENUM_DATA_V0,
            NTFS_VOLUME_DATA_BUFFER, READ_USN_JOURNAL_DATA_V0, USN_JOURNAL_DATA_V0, USN_RECORD_V2,
        },
        Threading::{GetCurrentProcess, OpenProcessToken},
    },
};

use crate::{
    Outcome, Result,
    corpus::Plan,
    mft::{self, Skip},
    tally::{RECORD_MASK, Tally, Volume},
};

// ---------------------------------------------------------------------------------------------
// Plumbing

struct Owned(HANDLE);

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateFileW/OpenProcessToken and is closed once, here.
        unsafe { CloseHandle(self.0) };
    }
}

fn os_error(what: &str) -> String {
    format!("{what}: {}", std::io::Error::last_os_error())
}

fn last_error() -> u32 {
    // SAFETY: reads the calling thread's last-error value.
    unsafe { GetLastError() }
}

/// 4 KiB-aligned bytes: non-cached volume reads need sector-aligned buffers.
#[repr(C, align(4096))]
#[derive(Clone, Copy)]
struct Page([u8; 4096]);

struct Buf(Vec<Page>);

impl Buf {
    fn new(bytes: usize) -> Self {
        Self(vec![Page([0; 4096]); bytes.div_ceil(4096)])
    }
    fn len(&self) -> usize {
        self.0.len() * 4096
    }
    fn len_u32(&self) -> u32 {
        u32::try_from(self.len()).unwrap_or(u32::MAX)
    }
    fn ptr(&mut self) -> *mut c_void {
        self.0.as_mut_ptr().cast()
    }
    fn bytes(&self) -> &[u8] {
        // SAFETY: `Page` is plain bytes and the Vec holds `len()` of them contiguously.
        unsafe { std::slice::from_raw_parts(self.0.as_ptr().cast(), self.len()) }
    }
    fn bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: as in `bytes`, borrowed mutably through `&mut self`.
        unsafe { std::slice::from_raw_parts_mut(self.0.as_mut_ptr().cast(), self.len()) }
    }
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}
fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap_or_default())
}
fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap_or_default())
}
fn units(b: &[u8]) -> impl Iterator<Item = u16> + '_ {
    b.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c))
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn nul(v: &[u16]) -> Vec<u16> {
    let mut z = Vec::with_capacity(v.len() + 1);
    z.extend_from_slice(v);
    z.push(0);
    z
}

/// `\\?\`-prefixed absolute path without a terminator, so long paths work everywhere.
fn long_path(p: &Path) -> Result<Vec<u16>> {
    let abs = std::path::absolute(p).map_err(|e| format!("{}: {e}", p.display()))?;
    let s: Vec<u16> = abs.as_os_str().encode_wide().collect();
    let mut s = if s.starts_with(&wide(r"\\")) {
        s
    } else {
        [wide(r"\\?\"), s].concat()
    };
    while s.len() > 7 && s.last() == Some(&u16::from(b'\\')) {
        s.pop();
    }
    Ok(s)
}

fn join(dir: &[u16], name: &[u16]) -> Vec<u16> {
    let mut p = Vec::with_capacity(dir.len() + 1 + name.len());
    p.extend_from_slice(dir);
    if p.last() != Some(&u16::from(b'\\')) {
        p.push(u16::from(b'\\'));
    }
    p.extend_from_slice(name);
    p
}

/// Opens with every share mode; `Err` is the Win32 error code.
fn open_raw(path: &[u16], access: u32, flags: u32) -> std::result::Result<Owned, u32> {
    let z = nul(path);
    // SAFETY: NUL-terminated path; no security attributes or template file.
    let h = unsafe {
        CreateFileW(
            z.as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null(),
            OPEN_EXISTING,
            flags,
            ptr::null_mut(),
        )
    };
    if h == INVALID_HANDLE_VALUE {
        Err(last_error())
    } else {
        Ok(Owned(h))
    }
}

fn open(path: &[u16], access: u32, flags: u32) -> Result<Owned> {
    open_raw(path, access, flags).map_err(|e| {
        format!(
            "{}: {}",
            String::from_utf16_lossy(path),
            std::io::Error::from_raw_os_error(e.cast_signed())
        )
    })
}

/// `\\.\X:` for the volume holding `dir` (drive letters only: enough for the spike).
pub fn volume_device(dir: &Path) -> Result<Vec<u16>> {
    let abs = std::path::absolute(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let z = nul(&abs.as_os_str().encode_wide().collect::<Vec<_>>());
    let mut out = [0u16; 300];
    // SAFETY: NUL-terminated input; output buffer length passed in characters.
    if unsafe { GetVolumePathNameW(z.as_ptr(), out.as_mut_ptr(), out.len() as u32) } == 0 {
        return Err(os_error("GetVolumePathNameW"));
    }
    let len = out.iter().position(|&c| c == 0).unwrap_or(0);
    let mount = String::from_utf16_lossy(&out[..len]);
    let letter = mount
        .strip_suffix('\\')
        .filter(|m| m.len() == 2 && m.ends_with(':'))
        .ok_or(format!("{mount}: not a drive letter"))?;
    Ok(wide(&format!(r"\\.\{letter}")))
}

fn ioctl_in_out(
    h: &Owned,
    code: u32,
    input: &[u8],
    out: &mut Buf,
) -> std::result::Result<usize, u32> {
    let mut got = 0u32;
    // SAFETY: input and output buffers are valid for the lengths passed; synchronous handle.
    let ok = unsafe {
        DeviceIoControl(
            h.0,
            code,
            if input.is_empty() {
                ptr::null()
            } else {
                input.as_ptr().cast()
            },
            input.len() as u32,
            out.ptr(),
            out.len_u32(),
            &raw mut got,
            ptr::null_mut(),
        )
    };
    if ok == 0 {
        Err(last_error())
    } else {
        Ok(got as usize)
    }
}

fn as_bytes<T: Copy>(v: &T) -> &[u8] {
    // SAFETY: `T` is a plain `#[repr(C)]` Win32 input struct; reading its bytes is fine.
    unsafe { std::slice::from_raw_parts(ptr::from_ref(v).cast(), size_of::<T>()) }
}

fn read_struct<T: Copy + Default>(b: &[u8]) -> T {
    let mut v = T::default();
    let n = size_of::<T>().min(b.len());
    // SAFETY: copies at most size_of::<T>() bytes into a plain-data `T`.
    unsafe { ptr::copy_nonoverlapping(b.as_ptr(), ptr::from_mut(&mut v).cast::<u8>(), n) };
    v
}

fn read_at(h: &Owned, offset: u64, buf: &mut [u8]) -> Result<()> {
    // SAFETY: plain seek on a handle this module opened.
    if unsafe { SetFilePointerEx(h.0, offset.cast_signed(), ptr::null_mut(), FILE_BEGIN) } == 0 {
        return Err(os_error("seek"));
    }
    let mut got = 0u32;
    // SAFETY: `buf` is writable for the length passed; synchronous read.
    let ok = unsafe {
        ReadFile(
            h.0,
            buf.as_mut_ptr(),
            buf.len() as u32,
            &raw mut got,
            ptr::null_mut(),
        )
    };
    if ok == 0 || got as usize != buf.len() {
        return Err(os_error(&format!("read {} bytes at {offset}", buf.len())));
    }
    Ok(())
}

fn file_index(path: &[u16]) -> Result<(u64, u32)> {
    let h = open(path, FILE_READ_ATTRIBUTES, FILE_FLAG_BACKUP_SEMANTICS)?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: valid handle and out struct.
    if unsafe { GetFileInformationByHandle(h.0, &raw mut info) } == 0 {
        return Err(os_error("GetFileInformationByHandle"));
    }
    let index = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
    Ok((index, info.nNumberOfLinks))
}

fn enable_privilege(name: &str) -> Result<()> {
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: pseudo-handle of this process; out pointer to a local.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES, &raw mut token) }
        == 0
    {
        return Err(os_error("OpenProcessToken"));
    }
    let token = Owned(token);
    let mut luid = LUID::default();
    let z = nul(&wide(name));
    // SAFETY: NUL-terminated privilege name; out pointer to a local.
    if unsafe { LookupPrivilegeValueW(ptr::null(), z.as_ptr(), &raw mut luid) } == 0 {
        return Err(os_error(name));
    }
    let tp = TOKEN_PRIVILEGES {
        PrivilegeCount: 1,
        Privileges: [LUID_AND_ATTRIBUTES {
            Luid: luid,
            Attributes: SE_PRIVILEGE_ENABLED,
        }],
    };
    // SAFETY: valid token handle; `tp` lives across the call; no previous state requested.
    let ok = unsafe {
        AdjustTokenPrivileges(
            token.0,
            0,
            &raw const tp,
            0,
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    if ok == 0 || last_error() == ERROR_NOT_ALL_ASSIGNED {
        return Err(format!("{name} not held by this token"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Directory walks: find, find-mt, dirinfo, dirinfo-mt

#[derive(Default)]
struct Local {
    tally: Tally,
    bytes: u64,
    arena: Vec<u16>,
    /// (file id, size) for hard-link-aware totals (dirinfo only).
    ids: Vec<(u128, u64)>,
}

impl Local {
    fn name(&mut self, name: &[u16]) {
        self.arena.extend_from_slice(name);
        self.tally.name_chars += name.len() as u64;
    }
}

fn is_dot(name: &[u16]) -> bool {
    name == [u16::from(b'.')] || name == [u16::from(b'.'), u16::from(b'.')]
}

fn list_find(dir: &[u16], local: &mut Local, subdirs: &mut Vec<Vec<u16>>) {
    let pattern = nul(&join(dir, &wide("*")));
    let mut data = WIN32_FIND_DATAW::default();
    // SAFETY: NUL-terminated pattern; `data` is the struct FindExInfoBasic fills.
    let h = unsafe {
        FindFirstFileExW(
            pattern.as_ptr(),
            FindExInfoBasic,
            (&raw mut data).cast(),
            FindExSearchNameMatch,
            ptr::null(),
            FIND_FIRST_EX_LARGE_FETCH,
        )
    };
    if h == INVALID_HANDLE_VALUE {
        if last_error() != ERROR_FILE_NOT_FOUND {
            local.tally.denied += 1;
        }
        return;
    }
    loop {
        let len = data
            .cFileName
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(data.cFileName.len());
        let name = &data.cFileName[..len];
        if !is_dot(name) {
            local.name(name);
            if data.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
                local.tally.dirs += 1;
                if data.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT == 0 {
                    subdirs.push(join(dir, name));
                }
            } else {
                local.tally.files += 1;
                local.bytes += (u64::from(data.nFileSizeHigh) << 32) | u64::from(data.nFileSizeLow);
            }
        }
        // SAFETY: live find handle; `data` is a valid out buffer.
        if unsafe { FindNextFileW(h, &raw mut data) } == 0 {
            break;
        }
    }
    // SAFETY: closes the handle FindFirstFileExW returned, once.
    unsafe { FindClose(h) };
}

/// One `FILE_ID_EXTD_DIR_INFO` entry.
struct Entry<'a> {
    name: &'a [u8],
    attributes: u32,
    size: u64,
    id: u128,
    write_time: u64,
}

/// Calls `f` for every entry of `dir` except `.` and `..`; `Err` carries the Win32 error.
fn for_each_entry(
    dir: &[u16],
    buf: &mut Buf,
    mut f: impl FnMut(Entry<'_>),
) -> std::result::Result<(), u32> {
    const NEXT: usize = offset_of!(FILE_ID_EXTD_DIR_INFO, NextEntryOffset);
    const WRITE: usize = offset_of!(FILE_ID_EXTD_DIR_INFO, LastWriteTime);
    const EOF: usize = offset_of!(FILE_ID_EXTD_DIR_INFO, EndOfFile);
    const ATTRS: usize = offset_of!(FILE_ID_EXTD_DIR_INFO, FileAttributes);
    const NAME_LEN: usize = offset_of!(FILE_ID_EXTD_DIR_INFO, FileNameLength);
    const ID: usize = offset_of!(FILE_ID_EXTD_DIR_INFO, FileId);
    const NAME: usize = offset_of!(FILE_ID_EXTD_DIR_INFO, FileName);
    let h = open_raw(dir, FILE_LIST_DIRECTORY, FILE_FLAG_BACKUP_SEMANTICS)?;
    loop {
        // SAFETY: `buf` is writable for its length and 8-aligned, as the entries need.
        let ok = unsafe {
            GetFileInformationByHandleEx(h.0, FileIdExtdDirectoryInfo, buf.ptr(), buf.len_u32())
        };
        if ok == 0 {
            let e = last_error();
            return if e == ERROR_NO_MORE_FILES {
                Ok(())
            } else {
                Err(e)
            };
        }
        let b = buf.bytes();
        let mut at = 0;
        loop {
            let name_len = u32_at(b, at + NAME_LEN) as usize;
            let name = &b[at + NAME..at + NAME + name_len];
            let dot = name == b".\0" || name == b".\0.\0";
            if !dot {
                f(Entry {
                    name,
                    attributes: u32_at(b, at + ATTRS),
                    size: u64_at(b, at + EOF),
                    id: u128::from_le_bytes(
                        b[at + ID..at + ID + 16].try_into().unwrap_or_default(),
                    ),
                    write_time: u64_at(b, at + WRITE),
                });
            }
            let next = u32_at(b, at + NEXT) as usize;
            if next == 0 {
                break;
            }
            at += next;
        }
    }
}

fn list_dirinfo(dir: &[u16], buf: &mut Buf, local: &mut Local, subdirs: &mut Vec<Vec<u16>>) {
    let mut name = Vec::new();
    let result = for_each_entry(dir, buf, |e| {
        name.clear();
        name.extend(units(e.name));
        local.name(&name);
        if e.attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
            local.tally.dirs += 1;
            if e.attributes & FILE_ATTRIBUTE_REPARSE_POINT == 0 {
                subdirs.push(join(dir, &name));
            }
        } else {
            local.tally.files += 1;
            local.bytes += e.size;
            local.ids.push((e.id, e.size));
        }
    });
    if result.is_err() {
        local.tally.denied += 1;
    }
}

/// Directory walk over `threads` workers sharing one queue of directories.
fn walk(root: &[u16], threads: usize, dirinfo: bool) -> Local {
    struct Queue {
        dirs: Vec<Vec<u16>>,
        busy: usize,
    }
    let queue = Mutex::new(Queue {
        dirs: vec![root.to_vec()],
        busy: 0,
    });
    let ready = Condvar::new();
    let worker = || {
        let mut local = Local::default();
        let mut buf = Buf::new(64 * 1024);
        let mut found = Vec::new();
        loop {
            let dir = {
                let mut q = queue
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                loop {
                    if let Some(d) = q.dirs.pop() {
                        q.busy += 1;
                        break Some(d);
                    }
                    if q.busy == 0 {
                        break None;
                    }
                    q = ready
                        .wait(q)
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                }
            };
            let Some(dir) = dir else {
                ready.notify_all();
                return local;
            };
            if dirinfo {
                list_dirinfo(&dir, &mut buf, &mut local, &mut found);
            } else {
                list_find(&dir, &mut local, &mut found);
            }
            let mut q = queue
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            q.busy -= 1;
            let woke = !found.is_empty() || q.busy == 0;
            q.dirs.append(&mut found);
            drop(q);
            if woke {
                ready.notify_all();
            }
        }
    };
    let locals: Vec<Local> = if threads <= 1 {
        vec![worker()]
    } else {
        std::thread::scope(|s| {
            let handles: Vec<_> = (0..threads).map(|_| s.spawn(worker)).collect();
            handles.into_iter().filter_map(|h| h.join().ok()).collect()
        })
    };
    let mut total = Local::default();
    for l in locals {
        total.tally.add(&l.tally);
        total.bytes += l.bytes;
        total.arena.extend_from_slice(&l.arena);
        total.ids.extend(l.ids);
    }
    total.tally.bytes = Some(total.bytes);
    total
}

fn scan_walk(root: &Path, threads: usize, dirinfo: bool) -> Result<Outcome> {
    let root = long_path(root)?;
    let start = Instant::now();
    let mut local = walk(&root, threads, dirinfo);
    let walked = start.elapsed();
    let mut phases = vec![("walk", walked)];
    if dirinfo {
        // Hard links share a file id: count each file once.
        let start = Instant::now();
        local.ids.sort_unstable_by_key(|&(id, _)| id);
        local.ids.dedup_by_key(|&mut (id, _)| id);
        local.tally.unique_bytes = Some(local.ids.iter().map(|&(_, s)| s).sum());
        phases.push(("dedup", start.elapsed()));
    }
    Ok(Outcome {
        tally: local.tally,
        phases,
        notes: Vec::new(),
    })
}

// ---------------------------------------------------------------------------------------------
// Whole-volume methods: usn, mft

fn open_volume(dir: &Path) -> Result<(Owned, u64)> {
    let root = long_path(dir)?;
    let (index, _) = file_index(&root)?;
    let device = volume_device(dir)?;
    let vol = open(&device, GENERIC_READ, 0)?;
    Ok((vol, index & RECORD_MASK))
}

fn usn_enum(vol: &Owned, volume: &mut Volume) -> std::result::Result<u64, u32> {
    const FRN: usize = offset_of!(USN_RECORD_V2, FileReferenceNumber);
    const PARENT: usize = offset_of!(USN_RECORD_V2, ParentFileReferenceNumber);
    const ATTRS: usize = offset_of!(USN_RECORD_V2, FileAttributes);
    const NAME_LEN: usize = offset_of!(USN_RECORD_V2, FileNameLength);
    const NAME_AT: usize = offset_of!(USN_RECORD_V2, FileNameOffset);
    let mut input = MFT_ENUM_DATA_V0 {
        StartFileReferenceNumber: 0,
        LowUsn: 0,
        HighUsn: i64::MAX,
    };
    let mut buf = Buf::new(1 << 20);
    let mut records = 0;
    loop {
        let got = match ioctl_in_out(vol, FSCTL_ENUM_USN_DATA, as_bytes(&input), &mut buf) {
            Ok(n) => n,
            Err(ERROR_HANDLE_EOF) => return Ok(records),
            Err(e) => return Err(e),
        };
        let b = &buf.bytes()[..got];
        input.StartFileReferenceNumber = u64_at(b, 0);
        let mut at = 8;
        while at + NAME_AT + 2 <= b.len() {
            let len = u32_at(b, at) as usize;
            if len == 0 || at + len > b.len() {
                break;
            }
            let r = &b[at..at + len];
            let frn = u64_at(r, FRN) & RECORD_MASK;
            let name_at = usize::from(u16_at(r, NAME_AT));
            let name = &r[name_at..name_at + usize::from(u16_at(r, NAME_LEN))];
            let rec = volume.record_mut(frn);
            rec.in_use = true;
            rec.is_dir = u32_at(r, ATTRS) & FILE_ATTRIBUTE_DIRECTORY != 0;
            volume.push_name(frn, u64_at(r, PARENT) & RECORD_MASK, units(name));
            records += 1;
            at += len;
        }
    }
}

fn scan_usn(dir: &Path) -> Result<Outcome> {
    let (vol, root) = open_volume(dir)?;
    let mut volume = Volume::default();
    let start = Instant::now();
    let records = usn_enum(&vol, &mut volume).map_err(|e| {
        format!(
            "FSCTL_ENUM_USN_DATA: {}",
            std::io::Error::from_raw_os_error(e.cast_signed())
        )
    })?;
    let enumerated = start.elapsed();
    let start = Instant::now();
    let tally = volume.tally(root, false);
    Ok(Outcome {
        tally,
        phases: vec![("enum", enumerated), ("tally", start.elapsed())],
        notes: vec![format!(
            "{records} USN records (one name per file: hard links beyond the first are missing)"
        )],
    })
}

fn ntfs_data(vol: &Owned) -> std::result::Result<NTFS_VOLUME_DATA_BUFFER, u32> {
    let mut buf = Buf::new(4096);
    ioctl_in_out(vol, FSCTL_GET_NTFS_VOLUME_DATA, &[], &mut buf)?;
    Ok(read_struct(buf.bytes()))
}

struct MftStats {
    records: u64,
    in_use: u64,
    bad: u64,
    bytes: u64,
    read: Duration,
    parse: Duration,
}

fn mft_read(vol: &Owned, volume: &mut Volume) -> Result<MftStats> {
    const CHUNK: u64 = 4 << 20;
    let data = ntfs_data(vol).map_err(|e| format!("FSCTL_GET_NTFS_VOLUME_DATA: error {e}"))?;
    let cluster = u64::from(data.BytesPerCluster);
    let rs = u64::from(data.BytesPerFileRecordSegment);
    if cluster == 0
        || rs == 0
        || !CHUNK.is_multiple_of(cluster)
        || !(cluster.is_multiple_of(rs) || rs.is_multiple_of(cluster))
    {
        return Err(format!(
            "unsupported geometry: cluster {cluster}, record {rs}"
        ));
    }
    let start_lcn = u64::try_from(data.MftStartLcn).map_err(|e| e.to_string())?;
    let mut first = Buf::new(cluster.max(rs) as usize);
    let n = (cluster.max(rs)) as usize;
    read_at(vol, start_lcn * cluster, &mut first.bytes_mut()[..n])?;
    let runs = mft::mft_runs(&mut first.bytes_mut()[..rs as usize])?;
    let total = u64::try_from(data.MftValidDataLength).unwrap_or(0) / rs;

    let mut stats = MftStats {
        records: 0,
        in_use: 0,
        bad: 0,
        bytes: 0,
        read: Duration::ZERO,
        parse: Duration::ZERO,
    };
    let mut buf = Buf::new(CHUNK as usize);
    let mut number = 0u64;
    'runs: for run in runs {
        let Some(lcn) = run.lcn else {
            number += run.clusters * cluster / rs; // sparse: no records stored there
            continue;
        };
        let mut offset = lcn * cluster;
        let mut left = run.clusters * cluster;
        while left > 0 {
            if number >= total {
                break 'runs;
            }
            let n = left.min(CHUNK) as usize;
            let t = Instant::now();
            read_at(vol, offset, &mut buf.bytes_mut()[..n])?;
            stats.read += t.elapsed();
            stats.bytes += n as u64;
            let t = Instant::now();
            for rec in buf.bytes_mut()[..n].chunks_exact_mut(rs as usize) {
                if number >= total {
                    break;
                }
                match mft::add_record(volume, number, rec) {
                    Ok(()) => stats.in_use += 1,
                    Err(Skip::Torn | Skip::Malformed | Skip::BadSignature) => stats.bad += 1,
                    Err(Skip::Empty | Skip::Free) => {}
                }
                number += 1;
            }
            stats.parse += t.elapsed();
            offset += n as u64;
            left -= n as u64;
        }
    }
    stats.records = number;
    Ok(stats)
}

fn scan_mft(dir: &Path) -> Result<Outcome> {
    let (vol, root) = open_volume(dir)?;
    let mut volume = Volume::default();
    let s = mft_read(&vol, &mut volume)?;
    let start = Instant::now();
    let tally = volume.tally(root, true);
    Ok(Outcome {
        tally,
        phases: vec![
            ("read", s.read),
            ("parse", s.parse),
            ("tally", start.elapsed()),
        ],
        notes: vec![format!(
            "{} records ({} in use, {} unreadable), {:.2} GiB read",
            s.records,
            s.in_use,
            s.bad,
            s.bytes as f64 / f64::from(1u32 << 30)
        )],
    })
}

/// Runs one method over `dir`.
pub fn scan(method: &str, dir: &Path, threads: usize) -> Result<Outcome> {
    match method {
        "find" => scan_walk(dir, 1, false),
        "find-mt" => scan_walk(dir, threads, false),
        "dirinfo" => scan_walk(dir, 1, true),
        "dirinfo-mt" => scan_walk(dir, threads, true),
        "usn" => scan_usn(dir),
        "mft" => scan_mft(dir),
        other => Err(format!("unknown method {other}")),
    }
}

/// Forced dismount: NTFS drops the volume's cached data; the next access mounts it again.
pub fn dismount(device: &[u16]) -> Result<()> {
    let vol = open(device, GENERIC_READ | GENERIC_WRITE, 0)?;
    let mut none = Buf::new(0);
    ioctl_in_out(&vol, FSCTL_DISMOUNT_VOLUME, &[], &mut none)
        .map(|_| ())
        .map_err(|e| format!("FSCTL_DISMOUNT_VOLUME: error {e}"))
}

// ---------------------------------------------------------------------------------------------
// Corpus

/// Creates the planned tree under `root` (which must not exist). Returns (dirs, files) time.
pub fn make_corpus(root: &Path, plan: &Plan, threads: usize) -> Result<(Duration, Duration)> {
    let base = long_path(root)?;
    let start = Instant::now();
    let mkdir = |p: &[u16]| {
        let z = nul(p);
        // SAFETY: NUL-terminated path; default security.
        if unsafe { CreateDirectoryW(z.as_ptr(), ptr::null()) } == 0 {
            return Err(os_error(&String::from_utf16_lossy(p)));
        }
        Ok(())
    };
    mkdir(&base)?;
    for d in &plan.dirs {
        mkdir(&join(&base, &wide(d)))?;
    }
    let dirs = start.elapsed();

    let start = Instant::now();
    let next = AtomicUsize::new(0);
    let failure = Mutex::new(None::<String>);
    std::thread::scope(|s| {
        for _ in 0..threads.max(1) {
            s.spawn(|| {
                loop {
                    let leaf = next.fetch_add(1, Ordering::Relaxed);
                    if leaf >= plan.leaves.len() {
                        return;
                    }
                    if let Err(e) = create_leaf(&base, plan, leaf) {
                        *failure
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(e);
                        next.store(usize::MAX / 2, Ordering::Relaxed);
                        return;
                    }
                }
            });
        }
    });
    if let Some(e) = failure
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
    {
        return Err(e);
    }
    Ok((dirs, start.elapsed()))
}

fn create_leaf(base: &[u16], plan: &Plan, leaf: usize) -> Result<()> {
    for f in plan.files(leaf) {
        let path = nul(&join(base, &wide(&f.path)));
        // SAFETY: NUL-terminated path; no security attributes or template.
        let h = unsafe {
            CreateFileW(
                path.as_ptr(),
                GENERIC_WRITE,
                0,
                ptr::null(),
                CREATE_NEW,
                FILE_ATTRIBUTE_NORMAL,
                ptr::null_mut(),
            )
        };
        if h == INVALID_HANDLE_VALUE {
            return Err(os_error(&f.path));
        }
        let h = Owned(h);
        if f.size > 0 {
            let eof = FILE_END_OF_FILE_INFO {
                EndOfFile: f.size.cast_signed(),
            };
            // SAFETY: valid handle; `eof` is the struct FileEndOfFileInfo expects.
            let ok = unsafe {
                SetFileInformationByHandle(
                    h.0,
                    FileEndOfFileInfo,
                    (&raw const eof).cast(),
                    size_of::<FILE_END_OF_FILE_INFO>() as u32,
                )
            };
            if ok == 0 {
                return Err(os_error(&format!("{}: set size", f.path)));
            }
        }
        drop(h); // the size reaches the directory entry on close, before the link copies it
        if let Some(link) = &f.link {
            let link = nul(&join(base, &wide(link)));
            // SAFETY: both paths NUL-terminated; default security.
            if unsafe { CreateHardLinkW(link.as_ptr(), path.as_ptr(), ptr::null()) } == 0 {
                return Err(os_error(&format!("{}: hard link", f.path)));
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Stale-plan detection (TD-0001 §12): USN cursor vs re-stat

fn query_journal(vol: &Owned) -> std::result::Result<USN_JOURNAL_DATA_V0, u32> {
    let mut buf = Buf::new(4096);
    ioctl_in_out(vol, FSCTL_QUERY_USN_JOURNAL, &[], &mut buf)?;
    Ok(read_struct(buf.bytes()))
}

/// Reads journal records after `from`; calls `f(frn, parent)` for each. Returns (records, bytes).
fn read_journal(
    vol: &Owned,
    journal: &USN_JOURNAL_DATA_V0,
    from: i64,
    mut f: impl FnMut(u64, u64),
) -> Result<(u64, u64)> {
    const FRN: usize = offset_of!(USN_RECORD_V2, FileReferenceNumber);
    const PARENT: usize = offset_of!(USN_RECORD_V2, ParentFileReferenceNumber);
    let mut input = READ_USN_JOURNAL_DATA_V0 {
        StartUsn: from,
        ReasonMask: u32::MAX,
        ReturnOnlyOnClose: 0,
        Timeout: 0,
        BytesToWaitFor: 0,
        UsnJournalID: journal.UsnJournalID,
    };
    let mut buf = Buf::new(1 << 20);
    let (mut records, mut bytes) = (0, 0);
    loop {
        let got = ioctl_in_out(vol, FSCTL_READ_USN_JOURNAL, as_bytes(&input), &mut buf)
            .map_err(|e| format!("FSCTL_READ_USN_JOURNAL: error {e}"))?;
        if got <= 8 {
            return Ok((records, bytes));
        }
        let b = &buf.bytes()[..got];
        input.StartUsn = u64_at(b, 0).cast_signed();
        bytes += got as u64 - 8;
        let mut at = 8;
        while at + 8 <= b.len() {
            let len = u32_at(b, at) as usize;
            if len == 0 || at + len > b.len() {
                break;
            }
            let r = &b[at..at + len];
            if u16_at(r, 4) == 2 {
                f(
                    u64_at(r, FRN) & RECORD_MASK,
                    u64_at(r, PARENT) & RECORD_MASK,
                );
            }
            records += 1;
            at += len;
        }
    }
}

struct Snapshot {
    path: Vec<u16>,
    frn: u64,
    size: u64,
    write_time: u64,
}

fn restat(files: &[Snapshot]) -> (Duration, usize) {
    let start = Instant::now();
    let mut changed = 0;
    for f in files {
        let mut data = WIN32_FILE_ATTRIBUTE_DATA::default();
        // SAFETY: NUL-terminated path; out struct of the standard info level.
        let ok = unsafe {
            GetFileAttributesExW(
                f.path.as_ptr(),
                GetFileExInfoStandard,
                (&raw mut data).cast(),
            )
        };
        let size = (u64::from(data.nFileSizeHigh) << 32) | u64::from(data.nFileSizeLow);
        let write = (u64::from(data.ftLastWriteTime.dwHighDateTime) << 32)
            | u64::from(data.ftLastWriteTime.dwLowDateTime);
        if ok == 0 || size != f.size || write != f.write_time {
            changed += 1;
        }
    }
    (start.elapsed(), changed)
}

/// Compares the two ways to tell that a plan's inputs changed between preview and apply.
#[allow(clippy::too_many_lines)] // one experiment, read top to bottom
pub fn fingerprint(dir: &Path, paths: usize, changes: usize) -> Result<String> {
    let root = long_path(dir)?;
    let mut out = format!("### stale-plan detection on {}\n\n", dir.display());

    // The plan: the first `paths` files of a dirinfo walk, with their ids (free from that walk).
    let start = Instant::now();
    let mut files = Vec::new();
    let mut dirs_seen = HashSet::new();
    let mut queue = vec![root.clone()];
    let mut buf = Buf::new(64 * 1024);
    while let Some(d) = queue.pop() {
        if files.len() >= paths {
            break;
        }
        if let Ok((index, _)) = file_index(&d) {
            dirs_seen.insert(index & RECORD_MASK);
        }
        let mut name = Vec::new();
        let _ = for_each_entry(&d, &mut buf, |e| {
            name.clear();
            name.extend(units(e.name));
            if e.attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
                queue.push(join(&d, &name));
            } else if files.len() < paths {
                files.push(Snapshot {
                    path: nul(&join(&d, &name)),
                    frn: (e.id as u64) & RECORD_MASK,
                    size: e.size,
                    write_time: e.write_time,
                });
            }
        });
    }
    let _ = writeln!(
        out,
        "plan: {} files in {} dirs, collected in {} ms (dirinfo walk; ids come free)",
        files.len(),
        dirs_seen.len(),
        start.elapsed().as_millis()
    );

    let (t, flagged) = restat(&files);
    let _ = writeln!(
        out,
        "re-stat, nothing changed: {} ms, {flagged} flagged",
        t.as_millis()
    );

    let device = volume_device(dir)?;
    let vol = open(&device, GENERIC_READ, 0)?;
    let t = Instant::now();
    let journal = query_journal(&vol)
        .map_err(|e| format!("FSCTL_QUERY_USN_JOURNAL: error {e} (1179: no journal)"))?;
    let query = t.elapsed();
    let _ = writeln!(
        out,
        "USN cursor: query {} µs (journal max {} MiB)",
        query.as_micros(),
        journal.MaximumSize >> 20
    );
    let cursor = journal.NextUsn;

    let t = Instant::now();
    let (records, _) = read_journal(&vol, &journal, cursor, |_, _| {})?;
    let _ = writeln!(
        out,
        "USN delta, nothing changed: {records} records in {} µs",
        t.elapsed().as_micros()
    );

    // Change `changes` plan files (append one byte), spread over the plan.
    let step = (files.len() / changes.max(1)).max(1);
    let mut targets = HashSet::new();
    for f in files.iter().step_by(step).take(changes) {
        // SAFETY: NUL-terminated path; no security attributes or template.
        let h = unsafe {
            CreateFileW(
                f.path.as_ptr(),
                FILE_APPEND_DATA,
                FILE_SHARE_READ,
                ptr::null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                ptr::null_mut(),
            )
        };
        if h == INVALID_HANDLE_VALUE {
            return Err(os_error("open a plan file to change it"));
        }
        let h = Owned(h);
        let mut wrote = 0u32;
        // SAFETY: one byte from a static buffer; synchronous handle.
        if unsafe { WriteFile(h.0, b"x".as_ptr(), 1, &raw mut wrote, ptr::null_mut()) } == 0 {
            return Err(os_error("append"));
        }
        targets.insert(f.frn);
    }
    let _ = writeln!(
        out,
        "changed {} plan files (one byte appended)",
        targets.len()
    );

    let plan_ids: HashSet<u64> = files.iter().map(|f| f.frn).collect();
    let t = Instant::now();
    let mut hits = HashSet::new();
    let (records, bytes) = read_journal(&vol, &journal, cursor, |frn, parent| {
        if plan_ids.contains(&frn) || dirs_seen.contains(&parent) {
            hits.insert(frn);
        }
    })?;
    let usn = t.elapsed();
    let detected = targets.iter().filter(|t| hits.contains(t)).count();
    let _ = writeln!(
        out,
        "USN delta: {records} records ({bytes} bytes) in {} µs, {detected}/{} changes detected",
        usn.as_micros(),
        targets.len()
    );

    let (t, flagged) = restat(&files);
    let _ = writeln!(
        out,
        "re-stat after: {} ms, {flagged}/{} flagged",
        t.as_millis(),
        targets.len()
    );
    Ok(out)
}

/// How much a USN delta costs on a live volume after `wait` of normal activity.
pub fn usn_noise(dir: &Path, wait: Duration) -> Result<String> {
    let device = volume_device(dir)?;
    let vol = open(&device, GENERIC_READ, 0)?;
    let journal = query_journal(&vol).map_err(|e| format!("FSCTL_QUERY_USN_JOURNAL: error {e}"))?;
    std::thread::sleep(wait);
    let t = Instant::now();
    let (records, bytes) = read_journal(&vol, &journal, journal.NextUsn, |_, _| {})?;
    Ok(format!(
        "USN noise on {}: {records} records ({bytes} bytes) after {} s idle, read in {} µs\n",
        String::from_utf16_lossy(&device),
        wait.as_secs(),
        t.elapsed().as_micros()
    ))
}

// ---------------------------------------------------------------------------------------------
// Probe: what this process may do on the volume of `dir` (runner VM vs container)

pub fn probe(dir: &Path) -> Result<String> {
    let mut out = format!("### probe {}\n\n", dir.display());
    let mut line = |step: &str, result: std::result::Result<String, String>| {
        let _ = match result {
            Ok(s) => writeln!(out, "- {step}: ok, {s}"),
            Err(e) => writeln!(out, "- {step}: FAILED, {e}"),
        };
    };
    let err = |e: u32| std::io::Error::from_raw_os_error(e.cast_signed()).to_string();
    line(
        "SeBackupPrivilege",
        enable_privilege("SeBackupPrivilege").map(|()| "enabled".into()),
    );
    let root = long_path(dir)?;
    line("FindFirstFileEx", {
        let mut l = Local::default();
        let mut sub = Vec::new();
        list_find(&root, &mut l, &mut sub);
        if l.tally.denied == 0 {
            Ok(format!("{} entries", l.tally.files + l.tally.dirs))
        } else {
            Err("listing failed".into())
        }
    });
    line("dirinfo", {
        let mut n = 0;
        for_each_entry(&root, &mut Buf::new(64 * 1024), |_| n += 1)
            .map(|()| format!("{n} entries"))
            .map_err(err)
    });
    let device = volume_device(dir)?;
    let vol = match open(&device, GENERIC_READ, 0) {
        Ok(v) => {
            line("open volume", Ok(String::from_utf16_lossy(&device)));
            v
        }
        Err(e) => {
            line("open volume", Err(e));
            return Ok(out);
        }
    };
    line(
        "NTFS volume data",
        ntfs_data(&vol)
            .map(|d| {
                format!(
                    "cluster {} B, record {} B, MFT {} records ({:.2} GiB valid)",
                    d.BytesPerCluster,
                    d.BytesPerFileRecordSegment,
                    d.MftValidDataLength / i64::from(d.BytesPerFileRecordSegment.max(1)),
                    d.MftValidDataLength as f64 / f64::from(1u32 << 30)
                )
            })
            .map_err(err),
    );
    line(
        "USN journal",
        query_journal(&vol)
            .map(|j| {
                format!(
                    "id {:x}, next USN {}, max {} MiB",
                    j.UsnJournalID,
                    j.NextUsn,
                    j.MaximumSize >> 20
                )
            })
            .map_err(err),
    );
    line("FSCTL_ENUM_USN_DATA (first 1 MiB)", {
        let input = MFT_ENUM_DATA_V0 {
            StartFileReferenceNumber: 0,
            LowUsn: 0,
            HighUsn: i64::MAX,
        };
        ioctl_in_out(
            &vol,
            FSCTL_ENUM_USN_DATA,
            as_bytes(&input),
            &mut Buf::new(1 << 20),
        )
        .map(|n| format!("{n} bytes"))
        .map_err(err)
    });
    line("$MFT as a file (backup semantics)", {
        let path = [wide(r"\\?\"), device[4..].to_vec(), wide(r"\$MFT")].concat();
        open(&path, GENERIC_READ, FILE_FLAG_BACKUP_SEMANTICS).and_then(|h| {
            let mut b = Buf::new(1 << 20);
            let len = b.len();
            read_at(&h, 0, &mut b.bytes_mut()[..len]).map(|()| "read 1 MiB".into())
        })
    });
    Ok(out)
}
