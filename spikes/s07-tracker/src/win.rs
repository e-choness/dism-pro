//! The Windows side: an ETW real-time session over the kernel file, registry and process
//! providers; a job-object launcher; the USN delta; registry last-write snapshots; the
//! `Global\_MSIExecute` poller; the fake installer; and the probe, bench and check commands that
//! combine them. Every `unsafe` block is one Win32 call on memory this module owns.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt::Write as _,
    fs,
    io::Write as _,
    mem::{offset_of, size_of},
    path::{Path, PathBuf},
    ptr,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use windows_sys::{
    Win32::{
        Foundation::{
            CloseHandle, ERROR_ALREADY_EXISTS, ERROR_CANCELLED, ERROR_FILE_NOT_FOUND,
            ERROR_INSUFFICIENT_BUFFER, ERROR_NO_MORE_ITEMS, ERROR_NOT_ALL_ASSIGNED, FILETIME,
            GENERIC_READ, GetLastError, HANDLE, INVALID_HANDLE_VALUE, LUID,
        },
        Security::{
            AdjustTokenPrivileges, LUID_AND_ATTRIBUTES, LookupPrivilegeValueW,
            SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES,
        },
        Storage::FileSystem::{
            CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_ID_DESCRIPTOR, FILE_ID_DESCRIPTOR_0,
            FILE_NAME_NORMALIZED, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
            FILE_SHARE_WRITE, FileIdType, GetFinalPathNameByHandleW, OPEN_EXISTING, OpenFileById,
            QueryDosDeviceW, VOLUME_NAME_DOS,
        },
        System::{
            Diagnostics::{
                Etw::{
                    CONTROLTRACE_HANDLE, CloseTrace, ControlTraceW,
                    EVENT_CONTROL_CODE_ENABLE_PROVIDER, EVENT_HEADER_FLAG_64_BIT_HEADER,
                    EVENT_PROPERTY_INFO, EVENT_RECORD, EVENT_TRACE_CONTROL_STOP,
                    EVENT_TRACE_LOGFILEW, EVENT_TRACE_LOGFILEW_0, EVENT_TRACE_LOGFILEW_1,
                    EVENT_TRACE_PROPERTIES, EVENT_TRACE_REAL_TIME_MODE, EnableTraceEx2,
                    EventKeywordInformation, OpenTraceW, PROCESS_TRACE_MODE_EVENT_RECORD,
                    PROCESS_TRACE_MODE_RAW_TIMESTAMP, PROCESS_TRACE_MODE_REAL_TIME,
                    PROVIDER_FIELD_INFO, PROVIDER_FIELD_INFOARRAY, ProcessTrace, StartTraceW,
                    TRACE_EVENT_INFO, TRACE_LEVEL_VERBOSE, TdhEnumerateProviderFieldInformation,
                    TdhGetEventInformation, WNODE_FLAG_TRACED_GUID,
                },
                ToolHelp::{
                    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                    TH32CS_SNAPPROCESS,
                },
            },
            IO::{CreateIoCompletionPort, DeviceIoControl, GetQueuedCompletionStatus, OVERLAPPED},
            Ioctl::{
                FSCTL_QUERY_USN_JOURNAL, FSCTL_READ_USN_JOURNAL, READ_USN_JOURNAL_DATA_V0,
                USN_JOURNAL_DATA_V0,
            },
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JOBOBJECT_ASSOCIATE_COMPLETION_PORT,
                JobObjectAssociateCompletionPortInformation, SetInformationJobObject,
                TerminateJobObject,
            },
            Performance::{QueryPerformanceCounter, QueryPerformanceFrequency},
            Registry::{
                HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_ENUMERATE_SUB_KEYS,
                KEY_QUERY_VALUE, KEY_SET_VALUE, KEY_WRITE, REG_DWORD, REG_OPTION_NON_VOLATILE,
                REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegDeleteValueW,
                RegEnumKeyExW, RegOpenKeyExW, RegQueryInfoKeyW, RegSetValueExW,
            },
            SystemServices::{JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO, JOB_OBJECT_MSG_NEW_PROCESS},
            Threading::{
                CREATE_NO_WINDOW, CREATE_SUSPENDED, CreateProcessW, GetCurrentProcess,
                GetCurrentProcessId, GetCurrentThread, GetExitCodeProcess, GetThreadTimes,
                OpenMutexW, OpenProcessToken, PROCESS_INFORMATION, ResumeThread, STARTUPINFOW,
            },
        },
    },
    core::GUID,
};

use crate::{
    Result,
    fixture::{
        self, ARP_KEY, FAKE_ARP_NAME, FAKE_RUN_VALUE, MSI_PRODUCT_CODE, RUN_KEY, hklm_subkey,
    },
    model::{self, Check, Kind, Op, Proc, RegDiff, Seen, Truth},
    tdh::{self, Layout, Prop, Value},
};

// ---------------------------------------------------------------------------------------------
// Plumbing

struct Owned(HANDLE);

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: the handle came from a Create*/Open* call and is closed once, here.
        unsafe { CloseHandle(self.0) };
    }
}

fn os_error(what: &str) -> String {
    format!("{what}: {}", std::io::Error::last_os_error())
}

fn win_error(what: &str, code: u32) -> String {
    format!("{what}: {}", std::io::Error::from_raw_os_error(code as i32))
}

/// NUL-terminated UTF-16.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

fn from_wide(units: &[u16]) -> String {
    let n = units.iter().position(|&c| c == 0).unwrap_or(units.len());
    String::from_utf16_lossy(&units[..n])
}

/// The NUL-terminated UTF-16 string at `offset` in `bytes` (0 = none).
fn wide_at(bytes: &[u8], offset: u32) -> String {
    let Some(rest) = bytes.get(offset as usize..).filter(|_| offset != 0) else {
        return String::new();
    };
    let units: Vec<u16> = rest
        .as_chunks::<2>()
        .0
        .iter()
        .map(|u| u16::from_le_bytes(*u))
        .take_while(|&u| u != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

fn filetime(ft: FILETIME) -> u64 {
    (u64::from(ft.dwHighDateTime) << 32) | u64::from(ft.dwLowDateTime)
}

pub fn qpc() -> i64 {
    let mut v = 0;
    // SAFETY: valid out pointer.
    unsafe { QueryPerformanceCounter(&raw mut v) };
    v
}

/// QPC ticks → milliseconds.
fn ms(ticks: i64) -> f64 {
    static FREQUENCY: OnceLock<i64> = OnceLock::new();
    let f = *FREQUENCY.get_or_init(|| {
        let mut f = 1;
        // SAFETY: valid out pointer.
        unsafe { QueryPerformanceFrequency(&raw mut f) };
        f.max(1)
    });
    ticks as f64 * 1000.0 / f as f64
}

fn timed<T>(f: impl FnOnce() -> T) -> (T, f64) {
    let t = Instant::now();
    let v = f();
    (v, t.elapsed().as_secs_f64() * 1000.0)
}

fn enable_privilege(name: &str) -> Result<()> {
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: current-process pseudo-handle; `token` is a valid out pointer.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES, &raw mut token) }
        == 0
    {
        return Err(os_error("OpenProcessToken"));
    }
    let token = Owned(token);
    let mut luid = LUID::default();
    let name = wide(name);
    // SAFETY: NUL-terminated name; `luid` is a valid out pointer.
    if unsafe { LookupPrivilegeValueW(ptr::null(), name.as_ptr(), &raw mut luid) } == 0 {
        return Err(os_error("LookupPrivilegeValueW"));
    }
    let tp = TOKEN_PRIVILEGES {
        PrivilegeCount: 1,
        Privileges: [LUID_AND_ATTRIBUTES {
            Luid: luid,
            Attributes: SE_PRIVILEGE_ENABLED,
        }],
    };
    // SAFETY: live token with adjust rights; `tp` is a valid one-entry TOKEN_PRIVILEGES.
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
    // SAFETY: reads the calling thread's last-error value.
    if ok == 0 || unsafe { GetLastError() } == ERROR_NOT_ALL_ASSIGNED {
        return Err(os_error("AdjustTokenPrivileges"));
    }
    Ok(())
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(f64::total_cmp);
    v.get(v.len() / 2).copied().unwrap_or(0.0)
}

// ---------------------------------------------------------------------------------------------
// ETW session and consumer

const SESSION: &str = "dpro-s07";
const INVALID_PROCESSTRACE: u64 = u64::MAX;
const MAX_OPS: usize = 2_000_000;
const SAMPLES: usize = 2;

const fn guid(data1: u32, data2: u16, data3: u16, data4: [u8; 8]) -> GUID {
    GUID {
        data1,
        data2,
        data3,
        data4,
    }
}

const KERNEL_FILE: GUID = guid(
    0xEDD0_8927,
    0x9CC4,
    0x4E65,
    [0xB9, 0x70, 0xC2, 0x56, 0x0F, 0xB5, 0xC2, 0x89],
);
const KERNEL_REGISTRY: GUID = guid(
    0x70EB_4F03,
    0xC1DE,
    0x4F73,
    [0xA0, 0x51, 0x33, 0xD1, 0x3D, 0x54, 0x13, 0xBD],
);
const KERNEL_PROCESS: GUID = guid(
    0x22FB_2CD6,
    0x0E7B,
    0x422B,
    [0xA0, 0xC7, 0x2F, 0xAD, 0x1F, 0xD0, 0xE7, 0x16],
);

fn same_guid(a: &GUID, b: &GUID) -> bool {
    a.data1 == b.data1 && a.data2 == b.data2 && a.data3 == b.data3 && a.data4 == b.data4
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Profile {
    /// File: names, writes, deletes, renames, new files. Registry: changes only.
    Lean,
    /// Lean plus every file open (`KERNEL_FILE_KEYWORD_CREATE`) and registry `OpenKey`.
    Full,
}

impl Profile {
    pub fn parse(s: &str) -> Result<Option<Self>> {
        match s {
            "none" => Ok(None),
            "lean" => Ok(Some(Self::Lean)),
            "full" => Ok(Some(Self::Full)),
            other => Err(format!("unknown mode {other:?} (none|lean|full)")),
        }
    }

    fn name(mode: Option<Self>) -> &'static str {
        match mode {
            None => "none",
            Some(Self::Lean) => "lean",
            Some(Self::Full) => "full",
        }
    }

    /// Keyword masks, from `logman query providers` (Windows 11 26300); `probe` lists the
    /// runner's.
    fn providers(self) -> [(&'static str, GUID, u64); 3] {
        let (file, registry) = match self {
            // FILENAME 0x10 | WRITE 0x200 | DELETE_PATH 0x400 | RENAME_SETLINK_PATH 0x800 |
            // CREATE_NEW_FILE 0x1000; SetSecurityKey 0x4 | SetInformationKey 0x40 |
            // SetValueKey 0x100 | DeleteValueKey 0x200 | CreateKey 0x1000 | DeleteKey 0x4000.
            Self::Lean => (0x1E10, 0x5344),
            // + CREATE 0x80; + OpenKey 0x2000.
            Self::Full => (0x1E90, 0x7344),
        };
        [
            ("Kernel-File", KERNEL_FILE, file),
            ("Kernel-Registry", KERNEL_REGISTRY, registry),
            // WINEVENT_KEYWORD_PROCESS
            ("Kernel-Process", KERNEL_PROCESS, 0x10),
        ]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub enum Source {
    File,
    Registry,
    Process,
}

impl Source {
    fn of(g: &GUID) -> Option<Self> {
        if same_guid(g, &KERNEL_FILE) {
            Some(Self::File)
        } else if same_guid(g, &KERNEL_REGISTRY) {
            Some(Self::Registry)
        } else if same_guid(g, &KERNEL_PROCESS) {
            Some(Self::Process)
        } else {
            None
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Registry => "registry",
            Self::Process => "process",
        }
    }
}

/// Indices of the properties the consumer reads, by name, per event layout.
#[derive(Clone, Copy, Default)]
struct Fields {
    file_name: Option<usize>,
    file_path: Option<usize>,
    file_object: Option<usize>,
    file_key: Option<usize>,
    base_object: Option<usize>,
    key_object: Option<usize>,
    status: Option<usize>,
    disposition: Option<usize>,
    base_name: Option<usize>,
    relative_name: Option<usize>,
    key_name: Option<usize>,
    value_name: Option<usize>,
    process_id: Option<usize>,
    parent_id: Option<usize>,
    image_name: Option<usize>,
}

struct Decoder {
    layout: Layout,
    fields: Fields,
}

impl Decoder {
    fn new(layout: Layout) -> Self {
        let i = |name: &str| layout.index(name);
        let fields = Fields {
            file_name: i("FileName"),
            file_path: i("FilePath"),
            file_object: i("FileObject"),
            file_key: i("FileKey"),
            base_object: i("BaseObject"),
            key_object: i("KeyObject"),
            status: i("Status"),
            disposition: i("Disposition"),
            base_name: i("BaseName"),
            relative_name: i("RelativeName"),
            key_name: i("KeyName"),
            value_name: i("ValueName"),
            process_id: i("ProcessID"),
            parent_id: i("ParentProcessID"),
            image_name: i("ImageName"),
        };
        Self { layout, fields }
    }
}

/// Everything the consumer learned. Owned by the consumer thread while the trace runs.
pub struct Collector {
    own_pid: u32,
    devices: Vec<(String, String)>,
    ready: Arc<AtomicBool>,
    /// Keep up to `SAMPLES` decoded events per id that name S07 (check only: formatting every
    /// event costs consumer CPU the bench measures).
    keep_samples: bool,
    decoders: HashMap<(Source, u16, u8), Option<Decoder>>,
    file_objects: HashMap<u64, String>,
    file_keys: HashMap<u64, String>,
    reg_objects: HashMap<u64, String>,
    pub events: u64,
    pub counts: BTreeMap<(Source, u16), u64>,
    pub tasks: BTreeMap<(Source, u16), String>,
    pub samples: BTreeMap<(Source, u16), Vec<String>>,
    pub ops: HashMap<Op, Seen>,
    pub dropped_ops: u64,
    pub unresolved_writes: HashMap<u32, u64>,
    pub procs: HashMap<u32, Proc>,
    pub tdh_failures: u64,
}

impl Collector {
    fn new(ready: Arc<AtomicBool>, keep_samples: bool) -> Self {
        Self {
            // SAFETY: no preconditions.
            own_pid: unsafe { GetCurrentProcessId() },
            devices: dos_devices(),
            ready,
            keep_samples,
            decoders: HashMap::new(),
            file_objects: HashMap::new(),
            file_keys: HashMap::new(),
            reg_objects: HashMap::new(),
            events: 0,
            counts: BTreeMap::new(),
            tasks: BTreeMap::new(),
            samples: BTreeMap::new(),
            ops: HashMap::new(),
            dropped_ops: 0,
            unresolved_writes: HashMap::new(),
            procs: HashMap::new(),
            tdh_failures: 0,
        }
    }

    fn on(&mut self, r: &EVENT_RECORD) {
        if self.events == 0 {
            self.ready.store(true, Ordering::Release);
        }
        self.events += 1;
        let h = &r.EventHeader;
        let Some(source) = Source::of(&h.ProviderId) else {
            return;
        };
        let id = h.EventDescriptor.Id;
        *self.counts.entry((source, id)).or_default() += 1;
        let wanted = match source {
            Source::File => matches!(id, 10 | 12 | 16 | 26 | 27 | 28 | 30),
            Source::Registry => matches!(id, 1 | 2 | 3 | 5 | 6 | 11 | 15),
            Source::Process => matches!(id, 1 | 2),
        };
        let pid = h.ProcessId;
        // Process starts of our own children are logged in our context: keep those.
        if !wanted || (pid == self.own_pid && source != Source::Process) {
            return;
        }
        let key = (source, id, h.EventDescriptor.Version);
        if !self.decoders.contains_key(&key) {
            let decoder = layout_of(r).map(Decoder::new);
            match &decoder {
                Some(d) => {
                    self.tasks
                        .entry((source, id))
                        .or_insert_with(|| d.layout.task.clone());
                }
                None => self.tdh_failures += 1,
            }
            self.decoders.insert(key, decoder);
        }
        let Some(Some(decoder)) = self.decoders.get(&key) else {
            return;
        };
        let pointer = if u32::from(h.Flags) & EVENT_HEADER_FLAG_64_BIT_HEADER == 0 {
            4
        } else {
            8
        };
        let values = tdh::decode(&decoder.layout, user_data(r), pointer);
        let fields = decoder.fields;
        let sample = (self.keep_samples
            && self.samples.get(&(source, id)).map_or(0, Vec::len) < SAMPLES)
            .then(|| describe(&decoder.layout.props, &values))
            .filter(|s| {
                s.as_bytes()
                    .windows(3)
                    .any(|w| w.eq_ignore_ascii_case(b"s07"))
            });
        if let Some(s) = sample {
            self.samples
                .entry((source, id))
                .or_default()
                .push(format!("pid {pid}: {s}"));
        }
        self.apply(source, id, pid, h.TimeStamp, &values, fields);
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one match over the event kinds reads best whole"
    )]
    fn apply(&mut self, source: Source, id: u16, pid: u32, t: i64, v: &[Option<Value>], f: Fields) {
        let text = |i: Option<usize>| i.and_then(|i| v.get(i)?.as_ref()?.text()).unwrap_or("");
        let int = |i: Option<usize>| i.and_then(|i| v.get(i)?.as_ref()?.int());
        match (source, id) {
            (Source::File, 10) => {
                if let Some(k) = int(f.file_key) {
                    let path = self.dos(text(f.file_name));
                    self.file_keys.insert(k, path);
                }
            }
            (Source::File, 12) => {
                if let Some(o) = int(f.file_object) {
                    let path = self.dos(text(f.file_name));
                    self.file_objects.insert(o, path);
                }
            }
            (Source::File, 30) => {
                let path = self.dos(text(f.file_name));
                if let Some(o) = int(f.file_object) {
                    self.file_objects.insert(o, path.clone());
                }
                self.op(pid, Kind::FileNew, path, String::new(), t);
            }
            (Source::File, 16) => {
                let path = int(f.file_object)
                    .and_then(|o| self.file_objects.get(&o))
                    .or_else(|| int(f.file_key).and_then(|k| self.file_keys.get(&k)))
                    .cloned();
                match path {
                    Some(p) => self.op(pid, Kind::FileWrite, p, String::new(), t),
                    None => *self.unresolved_writes.entry(pid).or_default() += 1,
                }
            }
            (Source::File, 26..=28) => {
                let kind = match id {
                    26 => Kind::FileDelete,
                    27 => Kind::FileRename,
                    _ => Kind::FileLink,
                };
                let path = self.dos(text(f.file_path));
                self.op(pid, kind, path, String::new(), t);
            }
            (Source::Registry, 1 | 2) => {
                if int(f.status) != Some(0) {
                    return;
                }
                let base_name = text(f.base_name);
                let base = if base_name.is_empty() {
                    int(f.base_object)
                        .and_then(|b| self.reg_objects.get(&b))
                        .cloned()
                        .unwrap_or_default()
                } else {
                    base_name.to_owned()
                };
                let key = model::join_key(&base, text(f.relative_name));
                if let Some(o) = int(f.key_object) {
                    self.reg_objects.insert(o, key.clone());
                }
                // Disposition 1 = REG_CREATED_NEW_KEY.
                if id == 1 && int(f.disposition) == Some(1) {
                    self.op(pid, Kind::KeyCreate, key, String::new(), t);
                }
            }
            (Source::Registry, _) => {
                if int(f.status) != Some(0) {
                    return;
                }
                let named = model::reg_path(text(f.key_name));
                let key = if model::is_absolute_key(&named) {
                    named
                } else {
                    int(f.key_object)
                        .and_then(|o| self.reg_objects.get(&o))
                        .cloned()
                        .unwrap_or(named)
                };
                let kind = match id {
                    3 => Kind::KeyDelete,
                    5 => Kind::ValueSet,
                    6 => Kind::ValueDelete,
                    11 => Kind::KeyInfo,
                    _ => Kind::KeySecurity,
                };
                let value = if matches!(id, 5 | 6) {
                    text(f.value_name).to_owned()
                } else {
                    String::new()
                };
                self.op(pid, kind, key, value, t);
            }
            (Source::Process, 1) => {
                if let Some(p) = int(f.process_id) {
                    let proc = Proc {
                        parent: int(f.parent_id).unwrap_or(0) as u32,
                        image: text(f.image_name).to_owned(),
                        start: Some(t),
                        stop: None,
                    };
                    self.procs.insert(p as u32, proc);
                }
            }
            (Source::Process, _) => {
                if let Some(p) = int(f.process_id) {
                    self.procs.entry(p as u32).or_default().stop = Some(t);
                }
            }
            (Source::File, _) => {}
        }
    }

    fn dos(&self, nt: &str) -> String {
        model::dos_path(nt, &self.devices)
    }

    fn op(&mut self, pid: u32, kind: Kind, path: String, value: String, t: i64) {
        let op = Op {
            pid,
            kind,
            path,
            value,
        };
        if let Some(seen) = self.ops.get_mut(&op) {
            seen.last = t;
            seen.count += 1;
        } else if self.ops.len() < MAX_OPS {
            self.ops.insert(
                op,
                Seen {
                    first: t,
                    last: t,
                    count: 1,
                },
            );
        } else {
            self.dropped_ops += 1;
        }
    }
}

fn describe(props: &[Prop], values: &[Option<Value>]) -> String {
    let mut s = String::new();
    for (p, v) in props.iter().zip(values) {
        let Some(v) = v else { continue };
        let shown = match v {
            Value::Int(n) if *n > 0xFFFF_FFFF => format!("{n:#x}"),
            Value::Int(n) => n.to_string(),
            Value::Str(t) => format!("{t:?}"),
            Value::Bytes(n) => format!("<{n} B>"),
        };
        let _ = write!(
            s,
            "{}{}={shown}",
            if s.is_empty() { "" } else { ", " },
            p.name
        );
    }
    s
}

fn user_data(r: &EVENT_RECORD) -> &[u8] {
    if r.UserData.is_null() || r.UserDataLength == 0 {
        return &[];
    }
    // SAFETY: ETW guarantees UserData points at UserDataLength bytes for the callback's duration,
    // and the slice doesn't outlive `r`.
    unsafe { std::slice::from_raw_parts(r.UserData.cast::<u8>(), usize::from(r.UserDataLength)) }
}

/// The event's layout from TDH (manifest-based providers).
fn layout_of(r: &EVENT_RECORD) -> Option<Layout> {
    let mut size = 0u32;
    // SAFETY: a size query: no buffer, a valid size pointer.
    let status =
        unsafe { TdhGetEventInformation(r, 0, ptr::null(), ptr::null_mut(), &raw mut size) };
    if status != ERROR_INSUFFICIENT_BUFFER {
        return None;
    }
    let mut buf = vec![0u64; (size as usize).div_ceil(8)];
    // SAFETY: `buf` holds at least `size` bytes, 8-aligned as TRACE_EVENT_INFO needs.
    let status = unsafe {
        TdhGetEventInformation(
            r,
            0,
            ptr::null(),
            buf.as_mut_ptr().cast::<TRACE_EVENT_INFO>(),
            &raw mut size,
        )
    };
    if status != 0 {
        return None;
    }
    // SAFETY: `buf` is initialized u64s; viewing them as `size` bytes (≤ the Vec's length) is sound.
    let bytes = unsafe { std::slice::from_raw_parts(buf.as_ptr().cast::<u8>(), size as usize) };
    // SAFETY: TDH filled a TRACE_EVENT_INFO at the start of the aligned buffer.
    let info = unsafe { &*buf.as_ptr().cast::<TRACE_EVENT_INFO>() };
    let base = offset_of!(TRACE_EVENT_INFO, EventPropertyInfoArray);
    let mut props = Vec::with_capacity(info.TopLevelPropertyCount as usize);
    for i in 0..info.TopLevelPropertyCount as usize {
        let at = base + i * size_of::<EVENT_PROPERTY_INFO>();
        if at + size_of::<EVENT_PROPERTY_INFO>() > bytes.len() {
            return None;
        }
        // SAFETY: in bounds (checked above); read_unaligned needs no alignment.
        let p =
            unsafe { ptr::read_unaligned(bytes.as_ptr().add(at).cast::<EVENT_PROPERTY_INFO>()) };
        // SAFETY: plain-integer union members; for a struct property they're read but unused
        // (the decoder stops at PropertyStruct).
        let (in_type, count, length) = unsafe {
            (
                p.Anonymous1.nonStructType.InType,
                p.Anonymous2.count,
                p.Anonymous3.length,
            )
        };
        props.push(Prop {
            name: wide_at(bytes, p.NameOffset),
            in_type,
            flags: p.Flags,
            count,
            length,
        });
    }
    Some(Layout {
        task: wide_at(bytes, info.TaskNameOffset),
        props,
    })
}

unsafe extern "system" fn on_event(record: *mut EVENT_RECORD) {
    // SAFETY: ETW passes a record valid for this call. UserContext is the Collector `consume`
    // registered; it's only used on this thread, inside ProcessTrace.
    let (record, collector) =
        unsafe { (&*record, &mut *(*record).UserContext.cast::<Collector>()) };
    collector.on(record);
}

#[repr(C)]
struct Properties {
    p: EVENT_TRACE_PROPERTIES,
    name: [u16; 1024],
}

fn properties() -> Box<Properties> {
    let mut b = Box::new(Properties {
        p: EVENT_TRACE_PROPERTIES::default(),
        name: [0; 1024],
    });
    b.p.Wnode.BufferSize = size_of::<Properties>() as u32;
    b.p.Wnode.Flags = WNODE_FLAG_TRACED_GUID;
    // QueryPerformanceCounter timestamps, kept raw by the consumer.
    b.p.Wnode.ClientContext = 1;
    b.p.LoggerNameOffset = offset_of!(Properties, name) as u32;
    b
}

fn stop_by_name() {
    let name = wide(SESSION);
    let mut props = properties();
    // SAFETY: stopping by name (null handle); `props` is a sized properties buffer. A missing
    // session is fine.
    unsafe {
        ControlTraceW(
            CONTROLTRACE_HANDLE { Value: 0 },
            name.as_ptr(),
            &raw mut props.p,
            EVENT_TRACE_CONTROL_STOP,
        )
    };
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EtwStats {
    pub events_lost: u32,
    pub buffers_lost: u32,
    pub buffers_written: u32,
}

struct Consumed {
    collector: Box<Collector>,
    cpu_ms: f64,
    status: u32,
}

pub struct Session {
    handle: CONTROLTRACE_HANDLE,
    consumer: Option<JoinHandle<Result<Consumed>>>,
}

impl Session {
    pub fn start(profile: Profile, samples: bool) -> Result<Self> {
        stop_by_name();
        let name = wide(SESSION);
        let mut props = properties();
        props.p.BufferSize = 256; // KiB
        props.p.MinimumBuffers = 64;
        props.p.MaximumBuffers = 1024;
        props.p.FlushTimer = 1;
        props.p.LogFileMode = EVENT_TRACE_REAL_TIME_MODE;
        let mut handle = CONTROLTRACE_HANDLE::default();
        // SAFETY: NUL-terminated name; `props` is sized and initialized as StartTraceW requires.
        let status = unsafe { StartTraceW(&raw mut handle, name.as_ptr(), &raw mut props.p) };
        if status != 0 {
            return Err(win_error(
                if status == ERROR_ALREADY_EXISTS {
                    "StartTraceW (session exists)"
                } else {
                    "StartTraceW"
                },
                status,
            ));
        }
        let mut session = Self {
            handle,
            consumer: None,
        };
        for (provider, id, keywords) in profile.providers() {
            // SAFETY: live session handle; `id` outlives the call; no extra parameters.
            let status = unsafe {
                EnableTraceEx2(
                    handle,
                    &raw const id,
                    EVENT_CONTROL_CODE_ENABLE_PROVIDER,
                    TRACE_LEVEL_VERBOSE as u8,
                    keywords,
                    0,
                    0,
                    ptr::null(),
                )
            };
            if status != 0 {
                return Err(win_error(&format!("EnableTraceEx2 {provider}"), status));
            }
        }
        let ready = Arc::new(AtomicBool::new(false));
        let collector = Box::new(Collector::new(Arc::clone(&ready), samples));
        session.consumer = Some(thread::spawn(move || consume(collector)));
        // System-wide providers are never quiet for long: the first event means we're consuming.
        let t = Instant::now();
        while !ready.load(Ordering::Acquire) && t.elapsed() < Duration::from_secs(5) {
            thread::sleep(Duration::from_millis(5));
        }
        Ok(session)
    }

    pub fn stop(mut self) -> Result<(Box<Collector>, EtwStats, f64)> {
        let mut props = properties();
        // SAFETY: live session handle; `props` receives the final statistics.
        let status = unsafe {
            ControlTraceW(
                self.handle,
                ptr::null(),
                &raw mut props.p,
                EVENT_TRACE_CONTROL_STOP,
            )
        };
        self.handle = CONTROLTRACE_HANDLE { Value: 0 };
        let totals = EtwStats {
            events_lost: props.p.EventsLost,
            buffers_lost: props.p.RealTimeBuffersLost,
            buffers_written: props.p.BuffersWritten,
        };
        let thread = self.consumer.take().ok_or("no consumer thread")?;
        let consumed = thread
            .join()
            .map_err(|_| "the consumer thread panicked".to_owned())??;
        if status != 0 {
            return Err(win_error("ControlTraceW stop", status));
        }
        // A stop ends ProcessTrace with 0; ERROR_CANCELLED is the other orderly end.
        if consumed.status != 0 && consumed.status != ERROR_CANCELLED {
            return Err(win_error("ProcessTrace", consumed.status));
        }
        Ok((consumed.collector, totals, consumed.cpu_ms))
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if self.handle.Value != 0 {
            let mut props = properties();
            // SAFETY: live session handle, stopped once; ends the consumer's ProcessTrace.
            unsafe {
                ControlTraceW(
                    self.handle,
                    ptr::null(),
                    &raw mut props.p,
                    EVENT_TRACE_CONTROL_STOP,
                )
            };
        }
    }
}

fn consume(mut collector: Box<Collector>) -> Result<Consumed> {
    let mut name = wide(SESSION);
    let mut log = EVENT_TRACE_LOGFILEW {
        LoggerName: name.as_mut_ptr(),
        Anonymous1: EVENT_TRACE_LOGFILEW_0 {
            ProcessTraceMode: PROCESS_TRACE_MODE_REAL_TIME
                | PROCESS_TRACE_MODE_EVENT_RECORD
                | PROCESS_TRACE_MODE_RAW_TIMESTAMP,
        },
        Anonymous2: EVENT_TRACE_LOGFILEW_1 {
            EventRecordCallback: Some(on_event),
        },
        Context: (&raw mut *collector).cast(),
        ..Default::default()
    };
    // SAFETY: `log` describes a real-time event-record consumer; `name` and `collector` outlive
    // the trace handle, which is closed below.
    let trace = unsafe { OpenTraceW(&raw mut log) };
    if trace.Value == INVALID_PROCESSTRACE {
        return Err(os_error("OpenTraceW"));
    }
    // SAFETY: one valid trace handle; callbacks run on this thread during this call.
    let status = unsafe { ProcessTrace(&raw const trace, 1, ptr::null(), ptr::null()) };
    // SAFETY: the handle from OpenTraceW, closed once.
    unsafe { CloseTrace(trace) };
    Ok(Consumed {
        collector,
        cpu_ms: thread_cpu_ms(),
        status,
    })
}

fn thread_cpu_ms() -> f64 {
    let (mut c, mut e, mut k, mut u) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    // SAFETY: the calling thread's pseudo-handle; valid out pointers.
    unsafe {
        GetThreadTimes(
            GetCurrentThread(),
            &raw mut c,
            &raw mut e,
            &raw mut k,
            &raw mut u,
        )
    };
    (filetime(k) + filetime(u)) as f64 / 10_000.0
}

/// A provider's keywords, from TDH: (name, mask).
fn keywords(id: &GUID) -> Result<Vec<(String, u64)>> {
    let mut size = 0u32;
    // SAFETY: a size query: no buffer, a valid size pointer.
    let status = unsafe {
        TdhEnumerateProviderFieldInformation(
            id,
            EventKeywordInformation,
            ptr::null_mut(),
            &raw mut size,
        )
    };
    if status != ERROR_INSUFFICIENT_BUFFER {
        return Err(win_error("TdhEnumerateProviderFieldInformation", status));
    }
    let mut buf = vec![0u64; (size as usize).div_ceil(8)];
    // SAFETY: `buf` holds at least `size` bytes, 8-aligned.
    let status = unsafe {
        TdhEnumerateProviderFieldInformation(
            id,
            EventKeywordInformation,
            buf.as_mut_ptr().cast::<PROVIDER_FIELD_INFOARRAY>(),
            &raw mut size,
        )
    };
    if status != 0 {
        return Err(win_error("TdhEnumerateProviderFieldInformation", status));
    }
    // SAFETY: initialized u64s viewed as `size` bytes (≤ the Vec's length).
    let bytes = unsafe { std::slice::from_raw_parts(buf.as_ptr().cast::<u8>(), size as usize) };
    // SAFETY: TDH filled a PROVIDER_FIELD_INFOARRAY at the start of the aligned buffer.
    let head = unsafe { &*buf.as_ptr().cast::<PROVIDER_FIELD_INFOARRAY>() };
    let base = offset_of!(PROVIDER_FIELD_INFOARRAY, FieldInfoArray);
    let mut out = Vec::new();
    for i in 0..head.NumberOfElements as usize {
        let at = base + i * size_of::<PROVIDER_FIELD_INFO>();
        if at + size_of::<PROVIDER_FIELD_INFO>() > bytes.len() {
            break;
        }
        // SAFETY: in bounds (checked above); read_unaligned needs no alignment.
        let f =
            unsafe { ptr::read_unaligned(bytes.as_ptr().add(at).cast::<PROVIDER_FIELD_INFO>()) };
        out.push((wide_at(bytes, f.NameOffset), f.Value));
    }
    Ok(out)
}

/// (`\Device\HarddiskVolumeN`, `C:`) for every drive letter in use.
fn dos_devices() -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut buf = vec![0u16; 1024];
    for letter in 'A'..='Z' {
        let drive = format!("{letter}:");
        let name = wide(&drive);
        // SAFETY: NUL-terminated name; `buf` holds `len` units.
        let n = unsafe { QueryDosDeviceW(name.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };
        if n > 0 {
            out.push((from_wide(&buf), drive));
        }
    }
    out
}

/// Running processes: pid → (parent, exe name).
fn process_table() -> HashMap<u32, Proc> {
    let mut out = HashMap::new();
    // SAFETY: no preconditions.
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snap == INVALID_HANDLE_VALUE {
        return out;
    }
    let snap = Owned(snap);
    let mut e = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    // SAFETY: live snapshot; `e` has dwSize set.
    let mut ok = unsafe { Process32FirstW(snap.0, &raw mut e) };
    while ok != 0 {
        let proc = Proc {
            parent: e.th32ParentProcessID,
            image: from_wide(&e.szExeFile),
            start: None,
            stop: None,
        };
        out.insert(e.th32ProcessID, proc);
        // SAFETY: as above.
        ok = unsafe { Process32NextW(snap.0, &raw mut e) };
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Job object launcher and the Windows Installer mutex

pub struct Launch {
    pub exit: u32,
    pub start: i64,
    pub empty: i64,
    pub job_pids: HashSet<u32>,
    pub timed_out: bool,
}

/// Starts `command` suspended inside a new job, resumes it, and waits until the job has no
/// processes left (`JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO`), collecting every process id it held.
fn run_in_job(command: &str, timeout: Duration) -> Result<Launch> {
    // SAFETY: an anonymous job with default security.
    let job = Owned(unsafe { CreateJobObjectW(ptr::null(), ptr::null()) });
    if job.0.is_null() {
        return Err(os_error("CreateJobObjectW"));
    }
    // SAFETY: a new completion port, not tied to a file.
    let port =
        Owned(unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, ptr::null_mut(), 0, 1) });
    if port.0.is_null() {
        return Err(os_error("CreateIoCompletionPort"));
    }
    let assoc = JOBOBJECT_ASSOCIATE_COMPLETION_PORT {
        CompletionKey: ptr::null_mut(),
        CompletionPort: port.0,
    };
    // SAFETY: the documented struct for this class, passed with its size.
    let ok = unsafe {
        SetInformationJobObject(
            job.0,
            JobObjectAssociateCompletionPortInformation,
            (&raw const assoc).cast(),
            size_of::<JOBOBJECT_ASSOCIATE_COMPLETION_PORT>() as u32,
        )
    };
    if ok == 0 {
        return Err(os_error("SetInformationJobObject"));
    }
    let mut line = wide(command);
    let si = STARTUPINFOW {
        cb: size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut pi = PROCESS_INFORMATION::default();
    // SAFETY: a mutable NUL-terminated command line; valid startup info and out struct.
    let ok = unsafe {
        CreateProcessW(
            ptr::null(),
            line.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            0,
            CREATE_SUSPENDED | CREATE_NO_WINDOW,
            ptr::null(),
            ptr::null(),
            &raw const si,
            &raw mut pi,
        )
    };
    if ok == 0 {
        return Err(os_error(&format!("CreateProcessW {command}")));
    }
    let process = Owned(pi.hProcess);
    let main_thread = Owned(pi.hThread);
    // SAFETY: both handles are live.
    if unsafe { AssignProcessToJobObject(job.0, process.0) } == 0 {
        let e = os_error("AssignProcessToJobObject");
        // SAFETY: live job handle.
        unsafe { TerminateJobObject(job.0, 1) };
        return Err(e);
    }
    let start = qpc();
    // SAFETY: the suspended main thread of the new process.
    unsafe { ResumeThread(main_thread.0) };
    let mut job_pids = HashSet::from([pi.dwProcessId]);
    let deadline = Instant::now() + timeout;
    let mut empty = None;
    while empty.is_none() {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        let (mut message, mut key, mut overlapped) = (0u32, 0usize, ptr::null_mut::<OVERLAPPED>());
        let wait = left.as_millis().min(1000) as u32;
        // SAFETY: live port; valid out pointers.
        let ok = unsafe {
            GetQueuedCompletionStatus(
                port.0,
                &raw mut message,
                &raw mut key,
                &raw mut overlapped,
                wait,
            )
        };
        if ok == 0 {
            continue;
        }
        match message {
            // The "overlapped" pointer carries the process id for these messages.
            JOB_OBJECT_MSG_NEW_PROCESS => {
                job_pids.insert(overlapped.addr() as u32);
            }
            JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO => empty = Some(qpc()),
            _ => {}
        }
    }
    let timed_out = empty.is_none();
    if timed_out {
        // SAFETY: live job handle.
        unsafe { TerminateJobObject(job.0, 1) };
    }
    let mut exit = 0u32;
    // SAFETY: live process handle; valid out pointer.
    unsafe { GetExitCodeProcess(process.0, &raw mut exit) };
    Ok(Launch {
        exit,
        start,
        empty: empty.unwrap_or_else(qpc),
        job_pids,
        timed_out,
    })
}

#[link(name = "ntdll", kind = "raw-dylib")]
unsafe extern "system" {
    fn NtQueryMutant(
        handle: HANDLE,
        class: u32,
        info: *mut MutantInfo,
        length: u32,
        returned: *mut u32,
    ) -> i32;
}

/// `MUTANT_BASIC_INFORMATION`.
#[repr(C)]
#[derive(Default)]
struct MutantInfo {
    current_count: i32,
    owned_by_caller: u8,
    abandoned: u8,
}

const MUTANT_QUERY_STATE: u32 = 0x0001;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MutexState {
    Absent,
    /// It exists but this process may not query it: the state is unknown.
    Denied,
    Free,
    Held,
}

/// The state of `Global\_MSIExecute`, read without acquiring it: acquiring it, even briefly,
/// could make a starting install fail with 1618 ("another installation is in progress").
fn msi_mutex() -> MutexState {
    let name = wide(r"Global\_MSIExecute");
    // SAFETY: NUL-terminated name.
    let h = unsafe { OpenMutexW(MUTANT_QUERY_STATE, 0, name.as_ptr()) };
    if h.is_null() {
        // SAFETY: reads the calling thread's last-error value.
        return if unsafe { GetLastError() } == ERROR_FILE_NOT_FOUND {
            MutexState::Absent
        } else {
            MutexState::Denied
        };
    }
    let h = Owned(h);
    let mut info = MutantInfo::default();
    let mut returned = 0u32;
    // SAFETY: live mutant handle with query access; `info` has MUTANT_BASIC_INFORMATION's layout.
    let status = unsafe {
        NtQueryMutant(
            h.0,
            0,
            &raw mut info,
            size_of::<MutantInfo>() as u32,
            &raw mut returned,
        )
    };
    if status < 0 || info.current_count > 0 {
        MutexState::Free
    } else {
        MutexState::Held
    }
}

/// Polls the mutex every 5 ms on a thread, recording each change with its QPC time.
struct MutexWatch {
    stop: Arc<AtomicBool>,
    thread: JoinHandle<Vec<(i64, MutexState)>>,
}

impl MutexWatch {
    fn start() -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            let mut changes: Vec<(i64, MutexState)> = Vec::new();
            while !flag.load(Ordering::Acquire) {
                let s = msi_mutex();
                if changes.last().is_none_or(|(_, last)| *last != s) {
                    changes.push((qpc(), s));
                }
                thread::sleep(Duration::from_millis(5));
            }
            changes
        });
        Self { stop, thread }
    }

    fn finish(self) -> Vec<(i64, MutexState)> {
        self.stop.store(true, Ordering::Release);
        self.thread.join().unwrap_or_default()
    }
}

fn wait_msi_idle(limit: Duration) {
    let t = Instant::now();
    while msi_mutex() == MutexState::Held && t.elapsed() < limit {
        thread::sleep(Duration::from_millis(10));
    }
}

// ---------------------------------------------------------------------------------------------
// USN delta

struct UsnSource {
    volume: Owned,
    root: Owned,
    journal: u64,
    start: i64,
}

pub struct UsnDelta {
    /// (path, merged reasons) per file or folder changed.
    pub files: Vec<(String, u32)>,
    pub records: usize,
    pub unresolved: usize,
    pub read_ms: f64,
    pub resolve_ms: f64,
}

fn open_path(path: &str, access: u32, flags: u32) -> Result<Owned> {
    let name = wide(path);
    // SAFETY: NUL-terminated path; no security attributes or template.
    let h = unsafe {
        CreateFileW(
            name.as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null(),
            OPEN_EXISTING,
            flags,
            ptr::null_mut(),
        )
    };
    if h == INVALID_HANDLE_VALUE {
        return Err(os_error(path));
    }
    Ok(Owned(h))
}

fn usn_cursor(volume: &Owned) -> Result<USN_JOURNAL_DATA_V0> {
    let mut data = USN_JOURNAL_DATA_V0::default();
    let mut got = 0u32;
    // SAFETY: live volume handle; `data` is the output struct, passed with its size.
    let ok = unsafe {
        DeviceIoControl(
            volume.0,
            FSCTL_QUERY_USN_JOURNAL,
            ptr::null(),
            0,
            (&raw mut data).cast(),
            size_of::<USN_JOURNAL_DATA_V0>() as u32,
            &raw mut got,
            ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(os_error("FSCTL_QUERY_USN_JOURNAL"));
    }
    Ok(data)
}

impl UsnSource {
    fn open(drive: char) -> Result<Self> {
        let volume = open_path(&format!(r"\\.\{drive}:"), GENERIC_READ, 0)?;
        let root = open_path(
            &format!(r"{drive}:\"),
            FILE_READ_ATTRIBUTES,
            FILE_FLAG_BACKUP_SEMANTICS,
        )?;
        let c = usn_cursor(&volume)?;
        Ok(Self {
            volume,
            root,
            journal: c.UsnJournalID,
            start: c.NextUsn,
        })
    }

    fn delta(&self) -> Result<UsnDelta> {
        let (read, read_ms) = timed(|| -> Result<_> {
            let end = usn_cursor(&self.volume)?.NextUsn;
            let mut buf = vec![0u64; 8192];
            let mut rd = READ_USN_JOURNAL_DATA_V0 {
                StartUsn: self.start,
                ReasonMask: u32::MAX,
                ReturnOnlyOnClose: 0,
                Timeout: 0,
                BytesToWaitFor: 0,
                UsnJournalID: self.journal,
            };
            let mut all = Vec::new();
            while rd.StartUsn < end {
                let mut got = 0u32;
                // SAFETY: live volume handle; input struct and output buffer passed with sizes.
                let ok = unsafe {
                    DeviceIoControl(
                        self.volume.0,
                        FSCTL_READ_USN_JOURNAL,
                        (&raw const rd).cast(),
                        size_of::<READ_USN_JOURNAL_DATA_V0>() as u32,
                        buf.as_mut_ptr().cast(),
                        (buf.len() * 8) as u32,
                        &raw mut got,
                        ptr::null_mut(),
                    )
                };
                if ok == 0 {
                    return Err(os_error("FSCTL_READ_USN_JOURNAL"));
                }
                // SAFETY: initialized u64s viewed as `got` bytes (≤ the buffer's size).
                let bytes =
                    unsafe { std::slice::from_raw_parts(buf.as_ptr().cast::<u8>(), got as usize) };
                let (next, recs) = model::parse_usn(bytes);
                let done = recs.is_empty() || next <= rd.StartUsn;
                all.extend(recs.into_iter().filter(|r| r.usn < end));
                rd.StartUsn = next;
                if done {
                    break;
                }
            }
            Ok(all)
        });
        let records = read?;
        let merged = model::merge_usn(&records);
        let ((files, unresolved), resolve_ms) = timed(|| {
            let mut dirs: HashMap<u64, Option<String>> = HashMap::new();
            let mut unresolved = 0;
            let files = merged
                .iter()
                .map(|f| {
                    let dir = dirs
                        .entry(f.parent)
                        .or_insert_with(|| path_by_id(&self.root, f.parent));
                    let path = if let Some(d) = dir {
                        format!(r"{}\{}", d.trim_end_matches('\\'), f.name)
                    } else {
                        unresolved += 1;
                        format!(r"<dir {:#x}>\{}", f.parent, f.name)
                    };
                    (path, f.reasons)
                })
                .collect();
            (files, unresolved)
        });
        Ok(UsnDelta {
            files,
            records: records.len(),
            unresolved,
            read_ms,
            resolve_ms,
        })
    }
}

/// The DOS path of a file or folder by its reference number (`OpenFileById`).
fn path_by_id(hint: &Owned, frn: u64) -> Option<String> {
    let desc = FILE_ID_DESCRIPTOR {
        dwSize: size_of::<FILE_ID_DESCRIPTOR>() as u32,
        Type: FileIdType,
        Anonymous: FILE_ID_DESCRIPTOR_0 { FileId: frn as i64 },
    };
    // SAFETY: live hint handle on the volume; `desc` is a valid descriptor.
    let h = unsafe {
        OpenFileById(
            hint.0,
            &raw const desc,
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null(),
            FILE_FLAG_BACKUP_SEMANTICS,
        )
    };
    if h == INVALID_HANDLE_VALUE {
        return None;
    }
    let h = Owned(h);
    let mut buf = vec![0u16; 2048];
    // SAFETY: live handle; `buf` holds `len` units.
    let n = unsafe {
        GetFinalPathNameByHandleW(
            h.0,
            buf.as_mut_ptr(),
            buf.len() as u32,
            FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
        )
    } as usize;
    if n == 0 || n >= buf.len() {
        return None;
    }
    let p = String::from_utf16_lossy(&buf[..n]);
    Some(p.strip_prefix(r"\\?\").map_or(p.clone(), str::to_owned))
}

// ---------------------------------------------------------------------------------------------
// Registry snapshots

/// The roots TD-0015 §5.7 watches. `HKCR` is a merged view of two of them, so it isn't walked.
const REG_ROOTS: [(HKEY, &str, &str); 3] = [
    (HKEY_LOCAL_MACHINE, "SOFTWARE", r"hklm\software"),
    (
        HKEY_LOCAL_MACHINE,
        r"SYSTEM\CurrentControlSet\Services",
        r"hklm\system\currentcontrolset\services",
    ),
    (HKEY_CURRENT_USER, "Software", r"hkcu\software"),
];

struct RegKey(HKEY);

impl Drop for RegKey {
    fn drop(&mut self) {
        // SAFETY: the key came from RegOpenKeyExW/RegCreateKeyExW and is closed once, here.
        unsafe { RegCloseKey(self.0) };
    }
}

impl RegKey {
    fn open(parent: HKEY, subkey: &str, access: u32) -> std::result::Result<Self, u32> {
        let name = wide(subkey);
        let mut h: HKEY = ptr::null_mut();
        // SAFETY: valid parent key; NUL-terminated subkey; `h` is a valid out pointer.
        let status = unsafe { RegOpenKeyExW(parent, name.as_ptr(), 0, access, &raw mut h) };
        if status == 0 {
            Ok(Self(h))
        } else {
            Err(status)
        }
    }

    /// Creates (or opens) a key below `HKEY_LOCAL_MACHINE`.
    fn create(subkey: &str) -> Result<Self> {
        let name = wide(subkey);
        let mut h: HKEY = ptr::null_mut();
        // SAFETY: NUL-terminated subkey; `h` is a valid out pointer; no class or security.
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_LOCAL_MACHINE,
                name.as_ptr(),
                0,
                ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE | KEY_QUERY_VALUE,
                ptr::null(),
                &raw mut h,
                ptr::null_mut(),
            )
        };
        if status != 0 {
            return Err(win_error(
                &format!(r"RegCreateKeyExW HKLM\{subkey}"),
                status,
            ));
        }
        Ok(Self(h))
    }

    fn set(&self, name: &str, kind: u32, data: &[u8]) -> Result<()> {
        let n = wide(name);
        // SAFETY: live key; NUL-terminated name; `data` is passed with its length.
        let status = unsafe {
            RegSetValueExW(
                self.0,
                n.as_ptr(),
                0,
                kind,
                data.as_ptr(),
                data.len() as u32,
            )
        };
        if status != 0 {
            return Err(win_error(&format!("RegSetValueExW {name}"), status));
        }
        Ok(())
    }

    fn set_sz(&self, name: &str, value: &str) -> Result<()> {
        let bytes: Vec<u8> = wide(value).iter().flat_map(|u| u.to_le_bytes()).collect();
        self.set(name, REG_SZ, &bytes)
    }
}

/// Every key below the roots: lowercase path → last-write FILETIME. Returns the denied count.
fn reg_snapshot() -> (HashMap<String, u64>, u32) {
    let mut out = HashMap::new();
    let mut denied = 0;
    for (hive, subkey, display) in REG_ROOTS {
        let Ok(key) = RegKey::open(hive, subkey, KEY_ENUMERATE_SUB_KEYS | KEY_QUERY_VALUE) else {
            denied += 1;
            continue;
        };
        let mut ft = FILETIME::default();
        // SAFETY: live key; only the last-write time is requested.
        unsafe {
            RegQueryInfoKeyW(
                key.0,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &raw mut ft,
            )
        };
        out.insert(display.to_owned(), filetime(ft));
        let mut path = display.to_owned();
        walk_key(&key, &mut path, 0, &mut out, &mut denied);
    }
    (out, denied)
}

fn walk_key(
    key: &RegKey,
    path: &mut String,
    depth: u32,
    out: &mut HashMap<String, u64>,
    denied: &mut u32,
) {
    if depth > 64 {
        return;
    }
    let mut children = Vec::new();
    let mut name = [0u16; 256];
    for index in 0u32.. {
        let mut len = name.len() as u32;
        let mut ft = FILETIME::default();
        // SAFETY: live key; `name` holds `len` units; valid out pointers.
        let status = unsafe {
            RegEnumKeyExW(
                key.0,
                index,
                name.as_mut_ptr(),
                &raw mut len,
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                &raw mut ft,
            )
        };
        if status == ERROR_NO_MORE_ITEMS {
            break;
        }
        if status != 0 {
            *denied += 1;
            break;
        }
        children.push((
            String::from_utf16_lossy(&name[..len as usize]),
            filetime(ft),
        ));
    }
    for (child, written) in children {
        let keep = path.len();
        path.push('\\');
        path.push_str(&child.to_lowercase());
        out.insert(path.clone(), written);
        match RegKey::open(key.0, &child, KEY_ENUMERATE_SUB_KEYS) {
            Ok(sub) => walk_key(&sub, path, depth + 1, out, denied),
            Err(_) => *denied += 1,
        }
        path.truncate(keep);
    }
}

pub struct RegChange {
    pub keys: usize,
    pub denied: u32,
    pub before_ms: f64,
    pub after_ms: f64,
    pub diff: RegDiff,
    pub before: HashMap<String, u64>,
}

// ---------------------------------------------------------------------------------------------
// Tracking one install

pub struct Options {
    pub profile: Option<Profile>,
    pub usn: bool,
    pub registry: bool,
    /// Keep sample events (see `Collector::keep_samples`).
    pub samples: bool,
    pub settle: Duration,
}

pub struct Etw {
    pub collector: Box<Collector>,
    pub stats: EtwStats,
    pub cpu_ms: f64,
    pub start_ms: f64,
}

pub struct Trace {
    pub launch: Launch,
    pub mutex: Vec<(i64, MutexState)>,
    pub etw: Option<Etw>,
    pub usn: Option<UsnDelta>,
    pub reg: Option<RegChange>,
    /// msiexec processes already running at launch (the service), pid → process.
    pub msiexec_before: HashMap<u32, Proc>,
}

impl Trace {
    pub fn install_ms(&self) -> f64 {
        ms(self.launch.empty - self.launch.start)
    }

    /// When the MSI mutex was last released after launch, in ms from launch.
    pub fn mutex_release_ms(&self) -> Option<f64> {
        let mut held = false;
        let mut release = None;
        for (t, s) in &self.mutex {
            if *s == MutexState::Held {
                held = true;
            } else if held && *t >= self.launch.start {
                release = Some(ms(t - self.launch.start));
            }
        }
        release
    }

    /// The process table attribution runs on: process starts seen by ETW, plus the msiexec
    /// processes that were already running (the service has no start event in the trace).
    fn processes(&self) -> HashMap<u32, Proc> {
        let mut procs = self.msiexec_before.clone();
        if let Some(etw) = &self.etw {
            procs.extend(etw.collector.procs.iter().map(|(k, v)| (*k, v.clone())));
        }
        procs
    }

    pub fn attributed(&self) -> HashSet<u32> {
        model::attributed(&self.processes(), &self.launch.job_pids)
    }
}

pub fn track(command: &str, opt: &Options) -> Result<Trace> {
    let reg_before = opt.registry.then(|| timed(reg_snapshot));
    let usn = if opt.usn {
        Some(UsnSource::open('C')?)
    } else {
        None
    };
    let msiexec_before = process_table()
        .into_iter()
        .filter(|(_, p)| model::is_msiexec(&p.image))
        .collect();
    let session = match opt.profile {
        Some(p) => {
            let (s, start_ms) = timed(|| Session::start(p, opt.samples));
            Some((s?, start_ms))
        }
        None => None,
    };
    let watch = MutexWatch::start();
    let launch = run_in_job(command, Duration::from_mins(15))?;
    // Windows Installer may still be finishing in its service after the client exits.
    wait_msi_idle(Duration::from_secs(120));
    thread::sleep(opt.settle);
    let etw = match session {
        Some((s, start_ms)) => {
            let (collector, stats, cpu_ms) = s.stop()?;
            Some(Etw {
                collector,
                stats,
                cpu_ms,
                start_ms,
            })
        }
        None => None,
    };
    let mutex = watch.finish();
    let usn = usn.map(|u| u.delta()).transpose()?;
    let reg = reg_before.map(|((before, denied), before_ms)| {
        let ((after, _), after_ms) = timed(reg_snapshot);
        RegChange {
            keys: before.len(),
            denied,
            before_ms,
            after_ms,
            diff: model::reg_diff(&before, &after),
            before,
        }
    });
    Ok(Trace {
        launch,
        mutex,
        etw,
        usn,
        reg,
        msiexec_before,
    })
}

// ---------------------------------------------------------------------------------------------
// The test installers

pub enum Installer {
    Fake {
        root: PathBuf,
        files: usize,
        keys: usize,
        modify: Option<PathBuf>,
    },
    Msi {
        msi: PathBuf,
        root: PathBuf,
        files: usize,
        keys: usize,
    },
}

impl Installer {
    fn describe(&self) -> String {
        match self {
            Self::Fake {
                root,
                files,
                keys,
                modify,
            } => format!(
                "fake installer (parent exits, a child writes): {files} files under `{}`, {keys} keys + `Run` value + uninstall entry{}",
                root.display(),
                if modify.is_some() {
                    ", appends to an existing file"
                } else {
                    ""
                }
            ),
            Self::Msi {
                root, files, keys, ..
            } => format!(
                "per-machine MSI (Windows Installer service writes): {files} files under `{}`, {keys} keys + `Run` value",
                root.display()
            ),
        }
    }

    fn command(&self) -> Result<String> {
        match self {
            Self::Fake {
                root,
                files,
                keys,
                modify,
            } => {
                let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
                let mut c = format!(
                    r#""{}" fake-install --root "{}" --files {files} --keys {keys}"#,
                    exe.display(),
                    root.display()
                );
                if let Some(m) = modify {
                    let _ = write!(c, r#" --modify "{}""#, m.display());
                }
                Ok(c)
            }
            Self::Msi { msi, .. } => Ok(format!(
                r#"msiexec.exe /i "{}" /qn /norestart"#,
                msi.display()
            )),
        }
    }

    fn root(&self) -> &Path {
        match self {
            Self::Fake { root, .. } | Self::Msi { root, .. } => root,
        }
    }

    fn truth(&self) -> Truth {
        match self {
            Self::Fake {
                root,
                files,
                keys,
                modify,
            } => fixture::fake_truth(root, *files, *keys, modify.as_deref()),
            Self::Msi {
                root, files, keys, ..
            } => fixture::msi_truth(root, *files, *keys),
        }
    }

    pub fn uninstall(&self) -> Result<()> {
        match self {
            Self::Fake { root, .. } => {
                if let Err(e) = fs::remove_dir_all(root)
                    && e.kind() != std::io::ErrorKind::NotFound
                {
                    return Err(format!("{}: {e}", root.display()));
                }
                for subkey in [
                    hklm_subkey(&fixture::data_key("fake")).to_owned(),
                    format!(r"{}\{FAKE_ARP_NAME}", hklm_subkey(ARP_KEY)),
                ] {
                    let name = wide(&subkey);
                    // SAFETY: NUL-terminated subkey of HKLM; a missing key is fine.
                    unsafe { RegDeleteTreeW(HKEY_LOCAL_MACHINE, name.as_ptr()) };
                }
                if let Ok(run) =
                    RegKey::open(HKEY_LOCAL_MACHINE, hklm_subkey(RUN_KEY), KEY_SET_VALUE)
                {
                    let value = wide(FAKE_RUN_VALUE);
                    // SAFETY: live key; NUL-terminated value name; a missing value is fine.
                    unsafe { RegDeleteValueW(run.0, value.as_ptr()) };
                }
                Ok(())
            }
            Self::Msi { .. } => {
                wait_msi_idle(Duration::from_secs(120));
                let status = std::process::Command::new("msiexec.exe")
                    .args(["/x", MSI_PRODUCT_CODE, "/qn", "/norestart"])
                    .status()
                    .map_err(|e| format!("msiexec /x: {e}"))?;
                wait_msi_idle(Duration::from_secs(120));
                // 1605: not installed; 3010: restart required.
                match status.code() {
                    Some(0 | 1605 | 3010) => Ok(()),
                    other => Err(format!("msiexec /x exited {other:?}")),
                }
            }
        }
    }
}

/// `tracker fake-install`: the parent starts a child and exits at once, as many bootstrappers
/// do; the child (`fake-child`) writes everything.
pub fn fake_install(
    root: &Path,
    files: usize,
    keys: usize,
    modify: Option<&Path>,
    child: bool,
) -> Result<()> {
    if !child {
        let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
        let mut cmd = std::process::Command::new(exe);
        cmd.arg("fake-child").arg("--root").arg(root);
        cmd.args(["--files", &files.to_string(), "--keys", &keys.to_string()]);
        if let Some(m) = modify {
            cmd.arg("--modify").arg(m);
        }
        cmd.spawn().map_err(|e| format!("start the child: {e}"))?;
        return Ok(());
    }
    // Let the parent exit first: the job must keep tracking the orphan.
    thread::sleep(Duration::from_millis(300));
    for i in 0..files {
        let path = root.join(fixture::file_rel(i));
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        fs::write(&path, vec![b'f'; fixture::file_size(i)])
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    for j in 0..keys {
        let key = RegKey::create(hklm_subkey(&fixture::numbered_key("fake", j)))?;
        key.set_sz("v", "x")?;
        key.set("n", REG_DWORD, &(j as u32).to_le_bytes())?;
    }
    let first = root.join(fixture::file_rel(0)).display().to_string();
    RegKey::create(hklm_subkey(RUN_KEY))?.set_sz(FAKE_RUN_VALUE, &first)?;
    let arp = RegKey::create(&format!(r"{}\{FAKE_ARP_NAME}", hklm_subkey(ARP_KEY)))?;
    arp.set_sz("DisplayName", "S07 fake installer")?;
    arp.set_sz("UninstallString", "tracker.exe fake-uninstall")?;
    if let Some(m) = modify {
        let mut f = fs::OpenOptions::new()
            .append(true)
            .open(m)
            .map_err(|e| format!("{}: {e}", m.display()))?;
        writeln!(f, "s07 fake install").map_err(|e| format!("{}: {e}", m.display()))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Commands

struct Row {
    mode: Option<Profile>,
    install_ms: f64,
    exit: u32,
    timed_out: bool,
    mutex_release_ms: Option<f64>,
    events: u64,
    lost: u32,
    buffers_lost: u32,
    cpu_ms: f64,
    start_ms: f64,
    unresolved: u64,
    dropped: u64,
    check: Option<Check>,
}

fn row(mode: Option<Profile>, trace: &Trace, truth: &Truth) -> Row {
    let mut r = Row {
        mode,
        install_ms: trace.install_ms(),
        exit: trace.launch.exit,
        timed_out: trace.launch.timed_out,
        mutex_release_ms: trace.mutex_release_ms(),
        events: 0,
        lost: 0,
        buffers_lost: 0,
        cpu_ms: 0.0,
        start_ms: 0.0,
        unresolved: 0,
        dropped: 0,
        check: None,
    };
    if let Some(etw) = &trace.etw {
        let attributed = trace.attributed();
        r.events = etw.collector.events;
        r.lost = etw.stats.events_lost;
        r.buffers_lost = etw.stats.buffers_lost;
        r.cpu_ms = etw.cpu_ms;
        r.start_ms = etw.start_ms;
        r.unresolved = attributed
            .iter()
            .filter_map(|p| etw.collector.unresolved_writes.get(p))
            .sum();
        r.dropped = etw.collector.dropped_ops;
        r.check = Some(model::check_etw(truth, &etw.collector.ops, &attributed));
    }
    r
}

/// Interleaved rounds of install → uninstall per tracing mode; reports the install time (launch
/// → job empty) per mode as a ratio to no tracing, and the trace's recall of the truth.
pub fn bench(inst: &Installer, runs: usize, modes: &[Option<Profile>]) -> Result<String> {
    let _ = enable_privilege("SeBackupPrivilege");
    let _ = inst.uninstall();
    let command = inst.command()?;
    let truth = inst.truth();
    let plain = Options {
        profile: None,
        usn: false,
        registry: false,
        samples: false,
        settle: Duration::ZERO,
    };
    // Warm-up, untraced: caches and the msiserver service start the same for every mode.
    let _ = track(&command, &plain)?;
    inst.uninstall()?;
    let mut rows = Vec::new();
    for round in 0..runs {
        for k in 0..modes.len() {
            let mode = modes[(round + k) % modes.len()];
            let opt = Options {
                profile: mode,
                usn: false,
                registry: false,
                samples: false,
                settle: Duration::from_secs(2),
            };
            let trace = track(&command, &opt)?;
            rows.push(row(mode, &trace, &truth));
            inst.uninstall()?;
        }
    }
    Ok(bench_report(inst, runs, modes, &rows))
}

#[allow(clippy::too_many_lines, reason = "a report: one table")]
fn bench_report(inst: &Installer, runs: usize, modes: &[Option<Profile>], rows: &[Row]) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "### {}\n", inst.describe());
    let _ = writeln!(
        s,
        "{runs} interleaved rounds after an untraced warm-up. Install = launch → job empty (ms). Recall = the worst run's.\n"
    );
    let _ = writeln!(
        s,
        "| mode | install median | ÷ none | min | max | MSI mutex released | events | lost (events/buffers) | consumer CPU | session start | files | modified | keys | values | unresolved writes |"
    );
    let _ = writeln!(
        s,
        "|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|---|---|---|---|--:|"
    );
    let base = {
        let mut v: Vec<f64> = rows
            .iter()
            .filter(|r| r.mode.is_none())
            .map(|r| r.install_ms)
            .collect();
        (!v.is_empty()).then(|| median(&mut v))
    };
    for mode in modes {
        let mine: Vec<&Row> = rows.iter().filter(|r| r.mode == *mode).collect();
        let mut t: Vec<f64> = mine.iter().map(|r| r.install_ms).collect();
        let med = median(&mut t);
        let ratio = base.map_or("-".into(), |b| format!("{:.2}", med / b));
        let mut rel: Vec<f64> = mine.iter().filter_map(|r| r.mutex_release_ms).collect();
        let rel = if rel.is_empty() {
            "-".into()
        } else {
            format!("{:.0} ms", median(&mut rel))
        };
        let mut ev: Vec<f64> = mine.iter().map(|r| r.events as f64).collect();
        let mut cpu: Vec<f64> = mine.iter().map(|r| r.cpu_ms).collect();
        let mut st: Vec<f64> = mine.iter().map(|r| r.start_ms).collect();
        let lost = mine.iter().map(|r| r.lost).max().unwrap_or(0);
        let blost = mine.iter().map(|r| r.buffers_lost).max().unwrap_or(0);
        let mut unres: Vec<f64> = mine.iter().map(|r| r.unresolved as f64).collect();
        let worst = mine
            .iter()
            .filter_map(|r| r.check.as_ref())
            .min_by_key(|c| c.files.found + c.modified.found + c.keys.found + c.values.found);
        let cells = worst.map_or_else(
            || ["-".to_owned(), "-".into(), "-".into(), "-".into()],
            |c| {
                [
                    c.files.cell(),
                    c.modified.cell(),
                    c.keys.cell(),
                    c.values.cell(),
                ]
            },
        );
        let etw = mode.is_some();
        let _ = writeln!(
            s,
            "| {} | {med:.0} | {ratio} | {:.0} | {:.0} | {rel} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            Profile::name(*mode),
            t.first().copied().unwrap_or(0.0),
            t.last().copied().unwrap_or(0.0),
            if etw {
                format!("{:.0}", median(&mut ev))
            } else {
                "-".into()
            },
            if etw {
                format!("{lost}/{blost}")
            } else {
                "-".into()
            },
            if etw {
                format!("{:.0} ms", median(&mut cpu))
            } else {
                "-".into()
            },
            if etw {
                format!("{:.0} ms", median(&mut st))
            } else {
                "-".into()
            },
            cells[0],
            cells[1],
            cells[2],
            cells[3],
            if etw {
                format!("{:.0}", median(&mut unres))
            } else {
                "-".into()
            },
        );
    }
    let bad: Vec<String> = rows
        .iter()
        .filter(|r| r.exit != 0 || r.timed_out || r.dropped > 0)
        .map(|r| {
            format!(
                "{}: exit {}, timed out {}, dropped ops {}",
                Profile::name(r.mode),
                r.exit,
                r.timed_out,
                r.dropped
            )
        })
        .collect();
    if !bad.is_empty() {
        let _ = writeln!(s, "\nIrregular runs: {}", bad.join("; "));
    }
    s
}

/// One install traced with ETW, the USN delta and the registry diff, compared item by item.
pub fn check(inst: &Installer, profile: Profile) -> Result<String> {
    enable_privilege("SeBackupPrivilege")?;
    let _ = inst.uninstall();
    let opt = Options {
        profile: Some(profile),
        usn: true,
        registry: true,
        samples: true,
        settle: Duration::from_secs(3),
    };
    let trace = track(&inst.command()?, &opt)?;
    let report = check_report(inst, profile, &trace, &inst.truth());
    inst.uninstall()?;
    Ok(report)
}

fn rel_ms(trace: &Trace, t: i64) -> f64 {
    ms(t - trace.launch.start)
}

#[allow(clippy::too_many_lines, reason = "a report: one section after another")]
fn check_report(inst: &Installer, profile: Profile, trace: &Trace, truth: &Truth) -> String {
    let mut s = String::new();
    let _ = writeln!(
        s,
        "### {} — profile {}\n",
        inst.describe(),
        Profile::name(Some(profile))
    );
    let _ = writeln!(
        s,
        "exit {}; job empty after {:.0} ms ({} processes in the job{})",
        trace.launch.exit,
        trace.install_ms(),
        trace.launch.job_pids.len(),
        if trace.launch.timed_out {
            ", TIMED OUT"
        } else {
            ""
        }
    );
    let changes: Vec<String> = trace
        .mutex
        .iter()
        .map(|(t, st)| format!("{:+.0} ms {st:?}", rel_ms(trace, *t)))
        .collect();
    let _ = writeln!(
        s,
        "\n`Global\\_MSIExecute` (relative to launch): {}",
        changes.join(", ")
    );

    let attributed = trace.attributed();
    let procs = trace.processes();
    if let Some(etw) = &trace.etw {
        let c = &etw.collector;
        let check = model::check_etw(truth, &c.ops, &attributed);
        let _ = writeln!(
            s,
            "\n#### ETW ({} events, lost {}/{} buffers of {} written, consumer CPU {:.0} ms, TDH failures {})",
            c.events,
            etw.stats.events_lost,
            etw.stats.buffers_lost,
            etw.stats.buffers_written,
            etw.cpu_ms,
            c.tdh_failures
        );
        let _ = writeln!(
            s,
            "\n| | files | modified | keys | values |\n|---|---|---|---|---|"
        );
        let _ = writeln!(
            s,
            "| recall (attributed) | {} | {} | {} | {} |",
            check.files.cell(),
            check.modified.cell(),
            check.keys.cell(),
            check.values.cell()
        );
        for (what, r) in [
            ("files", &check.files),
            ("modified", &check.modified),
            ("keys", &check.keys),
            ("values", &check.values),
        ] {
            if !r.missing.is_empty() {
                let some: Vec<&str> = r.missing.iter().take(5).map(String::as_str).collect();
                let _ = writeln!(
                    s,
                    "\nmissing {what} ({}): `{}`",
                    r.missing.len(),
                    some.join("`, `")
                );
            }
        }
        let _ = writeln!(s, "\nAttributed processes:");
        let mut pids: Vec<u32> = attributed.iter().copied().collect();
        pids.sort_unstable();
        for pid in pids {
            let p = procs.get(&pid);
            let _ = writeln!(
                s,
                "- {pid} `{}` parent {} {}{}",
                p.map_or("?", |p| model::file_name(&p.image)),
                p.map_or(0, |p| p.parent),
                if trace.launch.job_pids.contains(&pid) {
                    "in job"
                } else {
                    "outside the job"
                },
                p.and_then(|p| p.start).map_or(String::new(), |t| format!(
                    ", started {:+.0} ms",
                    rel_ms(trace, t)
                )),
            );
        }
        let writers = model::writers(truth, &c.ops);
        let names: Vec<String> = writers
            .iter()
            .map(|(pid, n)| {
                format!(
                    "{pid} `{}` ({n})",
                    procs.get(pid).map_or("?", |p| model::file_name(&p.image))
                )
            })
            .collect();
        let _ = writeln!(s, "\nTruth files written by: {}", names.join(", "));
        let mine = || c.ops.iter().filter(|(op, _)| attributed.contains(&op.pid));
        let first = mine().map(|(_, seen)| seen.first).min();
        let last = mine().map(|(_, seen)| seen.last).max();
        let after: Vec<&Op> = c
            .ops
            .iter()
            .filter(|(op, seen)| attributed.contains(&op.pid) && seen.last > trace.launch.empty)
            .map(|(op, _)| op)
            .collect();
        let _ = writeln!(
            s,
            "\nAttributed operations from {:+.0} ms to {:+.0} ms; {} after the job emptied{}",
            first.map_or(0.0, |t| rel_ms(trace, t)),
            last.map_or(0.0, |t| rel_ms(trace, t)),
            after.len(),
            if after.is_empty() {
                String::new()
            } else {
                format!(
                    " (e.g. {})",
                    after
                        .iter()
                        .take(3)
                        .map(|o| format!("{} `{}`", o.kind.name(), o.path))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        );
        let mut by_kind: BTreeMap<Kind, usize> = BTreeMap::new();
        for op in c.ops.keys().filter(|op| attributed.contains(&op.pid)) {
            *by_kind.entry(op.kind).or_default() += 1;
        }
        let kinds: Vec<String> = by_kind
            .iter()
            .map(|(k, n)| format!("{} {n}", k.name()))
            .collect();
        let _ = writeln!(s, "\nAttributed distinct operations: {}", kinds.join(", "));
        let files: Vec<&str> = c
            .ops
            .keys()
            .filter(|op| attributed.contains(&op.pid) && op.kind.is_file())
            .map(|op| op.path.as_str())
            .collect();
        let _ = writeln!(
            s,
            "\nAttributed file operations by folder: {}",
            model::top_folders(files.into_iter(), 8)
        );
        let unresolved: u64 = attributed
            .iter()
            .filter_map(|p| c.unresolved_writes.get(p))
            .sum();
        let _ = writeln!(s, "\nWrites with no known name (attributed): {unresolved}");
        let _ = writeln!(s, "\nEvent volume (id: task, count):");
        for ((src, id), n) in &c.counts {
            let task = c.tasks.get(&(*src, *id)).map_or("", String::as_str);
            let _ = writeln!(s, "- {} {id} {task}: {n}", src.name());
        }
        let _ = writeln!(s, "\nSamples (events naming S07):");
        for ((src, id), v) in &c.samples {
            for x in v {
                let _ = writeln!(s, "- {} {id}: {}", src.name(), truncate(x, 400));
            }
        }
    }

    if let Some(u) = &trace.usn {
        let paths: HashMap<String, u32> = u
            .files
            .iter()
            .map(|(p, r)| (p.to_lowercase(), *r))
            .collect();
        let found = truth
            .files
            .iter()
            .filter(|f| paths.contains_key(&f.to_lowercase()))
            .count();
        let data = model::USN_DATA_OVERWRITE | model::USN_DATA_EXTEND | model::USN_DATA_TRUNCATION;
        let modified = truth
            .modified
            .iter()
            .filter(|f| paths.get(&f.to_lowercase()).is_some_and(|r| r & data != 0))
            .count();
        let root = format!("{}\\", inst.root().display()).to_lowercase();
        let etw_paths: HashSet<String> = trace.etw.as_ref().map_or_else(HashSet::new, |e| {
            e.collector
                .ops
                .keys()
                .filter(|op| op.kind.is_file() && attributed.contains(&op.pid))
                .map(|op| op.path.to_lowercase())
                .collect()
        });
        let under_root = paths.keys().filter(|p| p.starts_with(&root)).count();
        let created = paths
            .values()
            .filter(|r| *r & model::USN_FILE_CREATE != 0)
            .count();
        let deleted = paths
            .values()
            .filter(|r| *r & model::USN_FILE_DELETE != 0)
            .count();
        let noise = paths
            .keys()
            .filter(|p| p.starts_with(r"c:\s07-noise\"))
            .count();
        let usn_only: Vec<&str> = u
            .files
            .iter()
            .map(|(p, _)| p.as_str())
            .filter(|p| !etw_paths.contains(&p.to_lowercase()))
            .collect();
        let _ = writeln!(
            s,
            "\n#### USN delta on C: ({} records, {} files/folders, read {:.1} ms, path resolve {:.1} ms, {} unresolved folders)",
            u.records,
            u.files.len(),
            u.read_ms,
            u.resolve_ms,
            u.unresolved
        );
        let _ = writeln!(
            s,
            "\n- truth files: {found}/{}; modified (data change): {modified}/{}",
            truth.files.len(),
            truth.modified.len()
        );
        let _ = writeln!(
            s,
            "- under the install folder: {under_root}; noise writer's folder: {noise}; created {created}, deleted {deleted}"
        );
        let _ = writeln!(
            s,
            "- not in the attributed ETW operations (would be Medium): {} — {}",
            usn_only.len(),
            model::top_folders(usn_only.into_iter(), 8)
        );
    }

    if let Some(r) = &trace.reg {
        let touched = r.diff.touched();
        let roots: Vec<String> = REG_ROOTS.iter().map(|(_, _, d)| (*d).to_owned()).collect();
        let key_found = truth
            .keys
            .iter()
            .filter(|k| r.diff.new.binary_search(&k.to_lowercase()).is_ok())
            .count();
        let value_found = truth
            .values
            .iter()
            .filter(|(k, _)| touched.contains(&k.to_lowercase()))
            .count();
        let mut items: Vec<String> = truth
            .keys
            .iter()
            .chain(truth.values.iter().map(|(k, _)| k))
            .map(|k| k.to_lowercase())
            .collect();
        items.sort();
        items.dedup();
        let unreachable: Vec<&String> = items
            .iter()
            .filter(|k| touched.contains(*k) && !model::pruned_reach(k, &touched, &roots))
            .collect();
        let run = RUN_KEY.to_lowercase();
        let _ = writeln!(
            s,
            "\n#### Registry diff ({} keys, {} denied; snapshot {:.0} ms before, {:.0} ms after)",
            r.keys, r.denied, r.before_ms, r.after_ms
        );
        let _ = writeln!(
            s,
            "\n- new {}, changed {}, deleted {}",
            r.diff.new.len(),
            r.diff.changed.len(),
            r.diff.deleted.len()
        );
        let _ = writeln!(
            s,
            "- truth keys new: {key_found}/{}; truth values' keys touched: {value_found}/{}",
            truth.keys.len(),
            truth.values.len()
        );
        let _ = writeln!(
            s,
            "- `Run` existed before: {}; touched: {}; ancestors touched: {}",
            r.before.contains_key(&run),
            touched.contains(&run),
            ancestors(&run)
                .iter()
                .filter(|a| touched.contains(*a))
                .count()
        );
        let _ = writeln!(
            s,
            "- a walk descending only into changed keys would miss {} of {} touched truth keys{}",
            unreachable.len(),
            items.iter().filter(|k| touched.contains(*k)).count(),
            if unreachable.is_empty() {
                String::new()
            } else {
                format!(
                    ": `{}`",
                    unreachable
                        .iter()
                        .take(4)
                        .map(|k| k.as_str())
                        .collect::<Vec<_>>()
                        .join("`, `")
                )
            }
        );
        let other: Vec<&str> = r
            .diff
            .new
            .iter()
            .chain(&r.diff.changed)
            .map(String::as_str)
            .filter(|k| !k.contains("s07"))
            .collect();
        let _ = writeln!(
            s,
            "- other keys touched during the install: {} — {}",
            other.len(),
            model::top_folders(other.into_iter(), 6)
        );
    }
    s
}

fn ancestors(key: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut k = key;
    while let Some((parent, _)) = k.rsplit_once('\\') {
        out.push(parent.to_owned());
        k = parent;
    }
    out
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_owned()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

/// What the providers offer here, and whether a session can run.
pub fn probe() -> String {
    let mut s = String::new();
    let _ = writeln!(
        s,
        "SeBackupPrivilege: {}",
        enable_privilege("SeBackupPrivilege").map_or_else(|e| e, |()| "enabled".into())
    );
    for (name, id, _) in Profile::Full.providers() {
        match keywords(&id) {
            Ok(k) => {
                let list: Vec<String> = k.iter().map(|(n, v)| format!("{v:#x} {n}")).collect();
                let _ = writeln!(s, "- {name} keywords: {}", list.join(", "));
            }
            Err(e) => {
                let _ = writeln!(s, "- {name} keywords: FAILED, {e}");
            }
        }
    }
    match Session::start(Profile::Full, false) {
        Ok(session) => {
            thread::sleep(Duration::from_secs(2));
            match session.stop() {
                Ok((c, stats, cpu)) => {
                    let _ = writeln!(
                        s,
                        "- 2 s full-profile session: {} events, lost {}/{} buffers, consumer CPU {cpu:.0} ms, TDH failures {}",
                        c.events, stats.events_lost, stats.buffers_lost, c.tdh_failures
                    );
                    for ((src, id), n) in &c.counts {
                        let task = c.tasks.get(&(*src, *id)).map_or("", String::as_str);
                        let _ = writeln!(s, "  - {} {id} {task}: {n}", src.name());
                    }
                }
                Err(e) => {
                    let _ = writeln!(s, "- session stop: FAILED, {e}");
                }
            }
        }
        Err(e) => {
            let _ = writeln!(s, "- session start: FAILED, {e}");
        }
    }
    match UsnSource::open('C') {
        Ok(u) => {
            let _ = writeln!(
                s,
                "- USN journal C:: id {:#x}, next USN {}",
                u.journal, u.start
            );
        }
        Err(e) => {
            let _ = writeln!(s, "- USN journal C:: FAILED, {e}");
        }
    }
    let msiexec: Vec<String> = process_table()
        .into_iter()
        .filter(|(_, p)| model::is_msiexec(&p.image))
        .map(|(pid, p)| format!("{pid} (parent {})", p.parent))
        .collect();
    let _ = writeln!(
        s,
        "- `Global\\_MSIExecute`: {:?}; msiexec processes: {}",
        msi_mutex(),
        if msiexec.is_empty() {
            "none".into()
        } else {
            msiexec.join(", ")
        }
    );
    let (keys, ms) = timed(reg_snapshot);
    let _ = writeln!(
        s,
        "- registry snapshot of the watched roots: {} keys, {} denied, {ms:.0} ms",
        keys.0.len(),
        keys.1
    );
    s
}
