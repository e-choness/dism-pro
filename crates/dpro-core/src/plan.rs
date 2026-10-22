//! The plan model (TD-0001 §5.1): an inert list of changes, previewed before anything is
//! applied. Plans hold no OS handles and are safe to serialize, diff and send to helpers.
//!
//! Changes are generic over their string type `S`: in memory `S = Atom` (indices into the
//! plan's [`Interner`]), and in the JSON document ([`PlanDoc`]) `S = String`.
//! [`ChangeKind::try_map`] converts between the two.

use serde::{Deserialize, Serialize};

use crate::{
    Error,
    id::TargetId,
    intern::{Atom, Interner},
    units::{Bytes, UnixMillis},
};

/// Current plan schema (TD-0001 §5.1, bumped with a reader for N-1).
pub const PLAN_SCHEMA: u16 = 1;

/// Windows build the plan was made against; re-checked at apply time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct OsBuild {
    pub major: u32,
    pub minor: u32,
    pub build: u32,
    /// Update build revision (UBR).
    pub revision: u32,
}

/// What produced the plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpDescriptor {
    /// Stable operation id, e.g. `clean.temp-files`.
    pub id: String,
    /// Fluent message id of the title.
    pub title_key: String,
    pub category: String,
    /// Whether a running job can stop mid-change (coarse native ops often can't).
    pub cancellable: bool,
}

/// Registry value data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum RegData<S> {
    Sz(S),
    ExpandSz(S),
    /// NUL-separated strings.
    MultiSz(S),
    Dword(u32),
    Qword(u64),
    /// Raw bytes, held in the arena.
    Binary(S),
}

/// Service start type (SCM `SERVICE_*_START` values 0–4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartType {
    Boot,
    System,
    Auto,
    Demand,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppxScope {
    CurrentUser,
    AllUsers,
    /// De-provision for new users.
    Provisioned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalKind {
    Uninstaller,
    Installer,
}

/// One mutation. Strings are `S`; see the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChangeKind<S> {
    /// Same-volume rename into the job's quarantine folder.
    FileQuarantine { path: S, bytes: Bytes, unique_bytes: Bytes },
    /// Always irreversible.
    FileDelete { path: S, bytes: Bytes, unique_bytes: Bytes },
    /// Reboot-time move into quarantine for files in use (TD-0014/0015 addendum).
    PendingQuarantine { path: S, dest: S },
    RegSet { key: S, value: S, new: RegData<S> },
    /// `value: None` deletes the whole key.
    RegDelete { key: S, value: Option<S> },
    ServiceStart { name: S, new: StartType },
    /// `captured_config` is the serialized config used to recreate the service on undo.
    ServiceDelete { name: S, captured_config: S },
    TaskState { path: S, enabled: bool },
    /// `xml` is the task definition, re-registered on undo.
    TaskDelete { path: S, xml: S },
    StartupEntry { id: S, enabled: bool },
    FirewallRuleDelete { rule: S },
    FeatureState { name: S, enable: bool, source: Option<S> },
    AppxRemove { full_name: S, scope: AppxScope },
    PackageAdd { path: S },
    PackageRemove { identity: S },
    DriverAdd { inf: S },
    DriverRemove { published: S },
    /// Expert-only; undo restores the full registry subtree in `keys`.
    MsiRegistrationDelete { product_code: S, keys: S },
    /// Coarse DISM/WIMGAPI call; `api` names it and `args` is its serialized arguments.
    Native { api: S, args: S },
    /// Third-party code. Always irreversible; journaled for audit only.
    ExternalProgram { command: S, kind: ExternalKind },
}

/// Whether and how a change can be undone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Reversibility<S> {
    /// The exact inverse is captured in the journal at apply time.
    Reversible,
    /// Reversible until a deadline, e.g. the quarantine TTL.
    ReversibleUntil { until: UnixMillis },
    /// Undone by applying `inverse`, best effort (e.g. re-enable a feature).
    ReversibleVia { inverse: Box<ChangeKind<S>> },
    /// `reason` is a Fluent message id explaining why.
    Irreversible { reason: S },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change<S> {
    pub kind: ChangeKind<S>,
    pub reversibility: Reversibility<S>,
    pub reboot_required: bool,
}

impl<S> ChangeKind<S> {
    /// Stable snake-case name, as used in JSON.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::FileQuarantine { .. } => "file_quarantine",
            Self::FileDelete { .. } => "file_delete",
            Self::PendingQuarantine { .. } => "pending_quarantine",
            Self::RegSet { .. } => "reg_set",
            Self::RegDelete { .. } => "reg_delete",
            Self::ServiceStart { .. } => "service_start",
            Self::ServiceDelete { .. } => "service_delete",
            Self::TaskState { .. } => "task_state",
            Self::TaskDelete { .. } => "task_delete",
            Self::StartupEntry { .. } => "startup_entry",
            Self::FirewallRuleDelete { .. } => "firewall_rule_delete",
            Self::FeatureState { .. } => "feature_state",
            Self::AppxRemove { .. } => "appx_remove",
            Self::PackageAdd { .. } => "package_add",
            Self::PackageRemove { .. } => "package_remove",
            Self::DriverAdd { .. } => "driver_add",
            Self::DriverRemove { .. } => "driver_remove",
            Self::MsiRegistrationDelete { .. } => "msi_registration_delete",
            Self::Native { .. } => "native",
            Self::ExternalProgram { .. } => "external_program",
        }
    }

    /// Kinds that can never be undone, whatever the caller claims (TD-0001 §5.1).
    #[must_use]
    pub fn is_always_irreversible(&self) -> bool {
        matches!(self, Self::FileDelete { .. } | Self::ExternalProgram { .. })
    }

    /// (bytes, unique bytes) this change frees; zero for non-file changes.
    #[must_use]
    pub fn freed(&self) -> (Bytes, Bytes) {
        match self {
            Self::FileQuarantine { bytes, unique_bytes, .. }
            | Self::FileDelete { bytes, unique_bytes, .. } => (*bytes, *unique_bytes),
            _ => (Bytes::ZERO, Bytes::ZERO),
        }
    }

    /// Converts every string with `f`, keeping everything else.
    ///
    /// # Errors
    /// The first error returned by `f`.
    pub fn try_map<T, E>(self, f: &mut impl FnMut(S) -> Result<T, E>) -> Result<ChangeKind<T>, E> {
        use ChangeKind as K;
        Ok(match self {
            K::FileQuarantine { path, bytes, unique_bytes } => {
                K::FileQuarantine { path: f(path)?, bytes, unique_bytes }
            }
            K::FileDelete { path, bytes, unique_bytes } => {
                K::FileDelete { path: f(path)?, bytes, unique_bytes }
            }
            K::PendingQuarantine { path, dest } => {
                K::PendingQuarantine { path: f(path)?, dest: f(dest)? }
            }
            K::RegSet { key, value, new } => {
                K::RegSet { key: f(key)?, value: f(value)?, new: new.try_map(f)? }
            }
            K::RegDelete { key, value } => {
                K::RegDelete { key: f(key)?, value: value.map(&mut *f).transpose()? }
            }
            K::ServiceStart { name, new } => K::ServiceStart { name: f(name)?, new },
            K::ServiceDelete { name, captured_config } => {
                K::ServiceDelete { name: f(name)?, captured_config: f(captured_config)? }
            }
            K::TaskState { path, enabled } => K::TaskState { path: f(path)?, enabled },
            K::TaskDelete { path, xml } => K::TaskDelete { path: f(path)?, xml: f(xml)? },
            K::StartupEntry { id, enabled } => K::StartupEntry { id: f(id)?, enabled },
            K::FirewallRuleDelete { rule } => K::FirewallRuleDelete { rule: f(rule)? },
            K::FeatureState { name, enable, source } => K::FeatureState {
                name: f(name)?,
                enable,
                source: source.map(&mut *f).transpose()?,
            },
            K::AppxRemove { full_name, scope } => K::AppxRemove { full_name: f(full_name)?, scope },
            K::PackageAdd { path } => K::PackageAdd { path: f(path)? },
            K::PackageRemove { identity } => K::PackageRemove { identity: f(identity)? },
            K::DriverAdd { inf } => K::DriverAdd { inf: f(inf)? },
            K::DriverRemove { published } => K::DriverRemove { published: f(published)? },
            K::MsiRegistrationDelete { product_code, keys } => {
                K::MsiRegistrationDelete { product_code: f(product_code)?, keys: f(keys)? }
            }
            K::Native { api, args } => K::Native { api: f(api)?, args: f(args)? },
            K::ExternalProgram { command, kind } => {
                K::ExternalProgram { command: f(command)?, kind }
            }
        })
    }
}

impl<S> RegData<S> {
    fn try_map<T, E>(self, f: &mut impl FnMut(S) -> Result<T, E>) -> Result<RegData<T>, E> {
        Ok(match self {
            Self::Sz(s) => RegData::Sz(f(s)?),
            Self::ExpandSz(s) => RegData::ExpandSz(f(s)?),
            Self::MultiSz(s) => RegData::MultiSz(f(s)?),
            Self::Dword(v) => RegData::Dword(v),
            Self::Qword(v) => RegData::Qword(v),
            Self::Binary(s) => RegData::Binary(f(s)?),
        })
    }
}

impl<S> Reversibility<S> {
    fn try_map<T, E>(self, f: &mut impl FnMut(S) -> Result<T, E>) -> Result<Reversibility<T>, E> {
        Ok(match self {
            Self::Reversible => Reversibility::Reversible,
            Self::ReversibleUntil { until } => Reversibility::ReversibleUntil { until },
            Self::ReversibleVia { inverse } => {
                Reversibility::ReversibleVia { inverse: Box::new((*inverse).try_map(f)?) }
            }
            Self::Irreversible { reason } => Reversibility::Irreversible { reason: f(reason)? },
        })
    }
}

impl<S> Change<S> {
    /// Converts every string with `f`.
    ///
    /// # Errors
    /// The first error returned by `f`.
    pub fn try_map<T, E>(self, f: &mut impl FnMut(S) -> Result<T, E>) -> Result<Change<T>, E> {
        Ok(Change {
            kind: self.kind.try_map(f)?,
            reversibility: self.reversibility.try_map(f)?,
            reboot_required: self.reboot_required,
        })
    }
}

/// Totals shown in the Preview drawer header, computed once when the plan is built.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanSummary {
    pub changes: u64,
    pub bytes: Bytes,
    /// Hard-link aware: bytes actually freed.
    pub unique_bytes: Bytes,
    pub irreversible: u64,
    pub reboot_required: bool,
    /// Earliest `ReversibleUntil` deadline, if any.
    pub earliest_deadline: Option<UnixMillis>,
}

impl PlanSummary {
    fn add<S>(&mut self, c: &Change<S>) {
        let (bytes, unique) = c.kind.freed();
        self.changes += 1;
        self.bytes = self.bytes.saturating_add(bytes);
        self.unique_bytes = self.unique_bytes.saturating_add(unique);
        self.reboot_required |= c.reboot_required;
        match &c.reversibility {
            Reversibility::Irreversible { .. } => self.irreversible += 1,
            Reversibility::ReversibleUntil { until } => {
                self.earliest_deadline = Some(self.earliest_deadline.map_or(*until, |d| d.min(*until)));
            }
            Reversibility::Reversible | Reversibility::ReversibleVia { .. } => {}
        }
    }
}

/// An inert, previewable list of changes against one target.
#[derive(Debug, Clone)]
pub struct Plan {
    pub target: TargetId,
    pub os_build: OsBuild,
    pub op: OpDescriptor,
    strings: Interner,
    changes: Vec<Change<Atom>>,
    summary: PlanSummary,
}

impl Plan {
    #[must_use]
    pub fn changes(&self) -> &[Change<Atom>] {
        &self.changes
    }

    #[must_use]
    pub fn strings(&self) -> &Interner {
        &self.strings
    }

    #[must_use]
    pub fn summary(&self) -> PlanSummary {
        self.summary
    }

    /// The JSON/export form, with every string resolved (lossy for invalid UTF-8).
    ///
    /// # Errors
    /// [`Error::UnknownAtom`] only if the plan is internally inconsistent.
    pub fn to_doc(&self) -> Result<PlanDoc, Error> {
        let mut resolve = |a: Atom| self.strings.resolve_lossy(a).map(std::borrow::Cow::into_owned);
        let changes = self
            .changes
            .iter()
            .map(|c| c.clone().try_map(&mut resolve))
            .collect::<Result<_, _>>()?;
        Ok(PlanDoc {
            schema: PLAN_SCHEMA,
            target: self.target,
            os_build: self.os_build,
            op: self.op.clone(),
            changes,
            summary: self.summary,
        })
    }

    /// Rebuilds a plan from its document. The summary is recomputed, never trusted.
    ///
    /// # Errors
    /// [`Error::Parse`] for an unknown schema; otherwise as [`PlanBuilder::push`].
    pub fn from_doc(doc: PlanDoc) -> Result<Self, Error> {
        if doc.schema != PLAN_SCHEMA {
            return Err(Error::Parse { what: "plan schema", value: doc.schema.to_string() });
        }
        let mut b = PlanBuilder::new(doc.target, doc.os_build, doc.op);
        for c in doc.changes {
            let c = c.try_map(&mut |s: String| b.strings.intern_str(&s))?;
            b.push(c)?;
        }
        Ok(b.finish())
    }
}

/// Serialized plan (`plan.v1`); the JSON schema is pinned in TD-0002.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanDoc {
    pub schema: u16,
    pub target: TargetId,
    pub os_build: OsBuild,
    pub op: OpDescriptor,
    pub changes: Vec<Change<String>>,
    pub summary: PlanSummary,
}

/// Builds a [`Plan`], enforcing the per-change invariants as changes are added.
#[derive(Debug)]
pub struct PlanBuilder {
    target: TargetId,
    os_build: OsBuild,
    op: OpDescriptor,
    strings: Interner,
    changes: Vec<Change<Atom>>,
    summary: PlanSummary,
}

impl PlanBuilder {
    #[must_use]
    pub fn new(target: TargetId, os_build: OsBuild, op: OpDescriptor) -> Self {
        Self {
            target,
            os_build,
            op,
            strings: Interner::new(),
            changes: Vec::new(),
            summary: PlanSummary::default(),
        }
    }

    /// Pre-sizes for `n` changes.
    pub fn reserve(&mut self, n: usize) {
        self.changes.reserve(n);
    }

    /// Interns a string for use in this plan's changes.
    ///
    /// # Errors
    /// [`Error::ArenaFull`].
    pub fn atom(&mut self, s: &str) -> Result<Atom, Error> {
        self.strings.intern_str(s)
    }

    /// Interns raw bytes (WTF-8 paths, binary registry data).
    ///
    /// # Errors
    /// [`Error::ArenaFull`].
    pub fn atom_bytes(&mut self, s: &[u8]) -> Result<Atom, Error> {
        self.strings.intern(s)
    }

    /// Adds a change.
    ///
    /// # Errors
    /// [`Error::MustBeIrreversible`] for a `FileDelete`/`ExternalProgram` change that isn't
    /// marked irreversible; [`Error::UnknownAtom`] for an atom from another plan.
    pub fn push(&mut self, change: Change<Atom>) -> Result<(), Error> {
        let index = self.changes.len();
        if change.kind.is_always_irreversible()
            && !matches!(change.reversibility, Reversibility::Irreversible { .. })
        {
            return Err(Error::MustBeIrreversible { index, kind: change.kind.name() });
        }
        // Validate atoms now so `to_doc` can't fail on a built plan.
        change.clone().try_map(&mut |a: Atom| {
            self.strings.get(a).map(|_| ()).ok_or(Error::UnknownAtom(a.raw()))
        })?;
        self.summary.add(&change);
        self.changes.push(change);
        Ok(())
    }

    #[must_use]
    pub fn finish(self) -> Plan {
        Plan {
            target: self.target,
            os_build: self.os_build,
            op: self.op,
            strings: self.strings,
            changes: self.changes,
            summary: self.summary,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op() -> OpDescriptor {
        OpDescriptor {
            id: "clean.temp-files".into(),
            title_key: "op-clean-temp".into(),
            category: "clean".into(),
            cancellable: true,
        }
    }

    fn builder() -> PlanBuilder {
        let build = OsBuild { major: 10, minor: 0, build: 26100, revision: 1 };
        PlanBuilder::new(TargetId([7; 16]), build, op())
    }

    #[test]
    fn change_kind_stays_small() {
        // Row storage instead of TD-0001's columnar layout relies on this (§7.3 budget).
        assert!(size_of::<ChangeKind<Atom>>() <= 32, "{}", size_of::<ChangeKind<Atom>>());
    }

    #[test]
    fn summary_totals_and_deadline() {
        let mut b = builder();
        let a = b.atom(r"C:\Windows\Temp\a.tmp").unwrap();
        let reason = b.atom("rev-file-delete").unwrap();
        b.push(Change {
            kind: ChangeKind::FileQuarantine { path: a, bytes: Bytes(100), unique_bytes: Bytes(60) },
            reversibility: Reversibility::ReversibleUntil { until: UnixMillis(50) },
            reboot_required: false,
        })
        .unwrap();
        b.push(Change {
            kind: ChangeKind::FileDelete { path: a, bytes: Bytes(10), unique_bytes: Bytes(10) },
            reversibility: Reversibility::Irreversible { reason },
            reboot_required: true,
        })
        .unwrap();
        let s = b.finish().summary();
        assert_eq!(s.changes, 2);
        assert_eq!(s.bytes, Bytes(110));
        assert_eq!(s.unique_bytes, Bytes(70));
        assert_eq!(s.irreversible, 1);
        assert!(s.reboot_required);
        assert_eq!(s.earliest_deadline, Some(UnixMillis(50)));
    }

    #[test]
    fn always_irreversible_kinds_are_enforced() {
        let mut b = builder();
        let cmd = b.atom("unins000.exe /SILENT").unwrap();
        let err = b
            .push(Change {
                kind: ChangeKind::ExternalProgram { command: cmd, kind: ExternalKind::Uninstaller },
                reversibility: Reversibility::Reversible,
                reboot_required: false,
            })
            .unwrap_err();
        assert!(matches!(err, Error::MustBeIrreversible { index: 0, kind: "external_program" }));
    }

    #[test]
    fn foreign_atoms_are_rejected() {
        let mut other = Interner::new();
        for i in 0..10 {
            other.intern_str(&i.to_string()).unwrap();
        }
        let foreign = other.intern_str("9").unwrap();
        let mut b = builder();
        let err = b
            .push(Change {
                kind: ChangeKind::PackageRemove { identity: foreign },
                reversibility: Reversibility::Reversible,
                reboot_required: false,
            })
            .unwrap_err();
        assert!(matches!(err, Error::UnknownAtom(9)));
    }

    #[test]
    fn json_round_trip_preserves_the_plan() {
        let mut b = builder();
        let key = b.atom(r"HKLM\SOFTWARE\Example").unwrap();
        let value = b.atom("Enabled").unwrap();
        let feature = b.atom("SMB1Protocol").unwrap();
        b.push(Change {
            kind: ChangeKind::RegSet { key, value, new: RegData::Dword(0) },
            reversibility: Reversibility::Reversible,
            reboot_required: false,
        })
        .unwrap();
        b.push(Change {
            kind: ChangeKind::FeatureState { name: feature, enable: false, source: None },
            reversibility: Reversibility::ReversibleVia {
                inverse: Box::new(ChangeKind::FeatureState { name: feature, enable: true, source: None }),
            },
            reboot_required: true,
        })
        .unwrap();
        let plan = b.finish();

        let doc = plan.to_doc().unwrap();
        let json = serde_json::to_string(&doc).unwrap();
        assert!(json.contains(r#""kind":"reg_set""#));
        assert!(json.contains(r#""type":"dword","data":0"#));
        let back: PlanDoc = serde_json::from_str(&json).unwrap();
        assert_eq!(back, doc);
        let rebuilt = Plan::from_doc(back).unwrap();
        assert_eq!(rebuilt.summary(), plan.summary());
        assert_eq!(rebuilt.to_doc().unwrap(), doc);
    }

    #[test]
    fn unknown_schema_is_rejected() {
        let mut doc = builder().finish().to_doc().unwrap();
        doc.schema = 99;
        assert!(Plan::from_doc(doc).is_err());
    }
}
