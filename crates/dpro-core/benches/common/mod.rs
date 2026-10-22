//! Shared fixtures for the dpro-core benches.

use dpro_core::{
    id::TargetId,
    plan::{Change, ChangeKind, OpDescriptor, OsBuild, Plan, PlanBuilder, Reversibility},
    units::{Bytes, UnixMillis},
};

/// Realistic temp-cleanup paths: ~90 UTF-16 units on average (TD-0001 §7.3).
pub fn path(i: usize) -> String {
    format!(
        r"C:\Users\someone\AppData\Local\Temp\{:04}\Microsoft.Build.Tasks.Core\{i:08}\obj\cache.tmp",
        i % 1000
    )
}

/// A plan of `n` file quarantines (the dominant change kind in cleanup).
pub fn file_plan(paths: &[String]) -> Plan {
    let build = OsBuild { major: 10, minor: 0, build: 26100, revision: 1 };
    let op = OpDescriptor {
        id: "clean.temp-files".into(),
        title_key: "op-clean-temp".into(),
        category: "clean".into(),
        cancellable: true,
    };
    let mut b = PlanBuilder::new(TargetId([1; 16]), build, op);
    b.reserve(paths.len());
    for (i, p) in paths.iter().enumerate() {
        let Ok(path) = b.atom(p) else { break };
        let size = Bytes(4096 + (i as u64 % 7) * 512);
        let change = Change {
            kind: ChangeKind::FileQuarantine { path, bytes: size, unique_bytes: size },
            reversibility: Reversibility::ReversibleUntil { until: UnixMillis(1_800_000_000_000) },
            reboot_required: false,
        };
        if b.push(change).is_err() {
            break;
        }
    }
    b.finish()
}
