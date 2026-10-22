# Caveats: written without a compiler (delete after the local clean-up)

Everything on this branch since `a2f0a20` was written in a cloud session with **no Docker, no cargo,
no Windows**. Nothing has been compiled, run, or measured. This file lists every known risk so the
local clean-up can go through it in one pass. Tick items as they're resolved; delete the file when
it's empty.

## 0. Run order for the clean-up
1. `docker compose build fmt test-win`
2. `docker compose run --rm lock` → commit `Cargo.lock` (the committed one is stale: new deps since 1.2)
3. `scripts/dev.ps1 fmt -- --fix` → commit (rustfmt was never run)
4. `fmt` → `lint` → `test` → `test-win` → `deny` → `build`
5. `ffi-check` with `DPRO_FFI_INCLUDE` = the ADK's `…\Deployment Tools\SDKs\DismApi\Include` (bind-mount it)
6. `bench -- plan_build`, `plan_serde`, `journal_append` → baseline numbers into TD-0001 §7
7. Open a PR so CI runs (CI never ran: it triggers only on PRs and `master`)

## 1. Everywhere
- [ ] **Clippy `-D warnings` with `pedantic`** was satisfied by eye only. Expect a handful of lints
      (`doc_markdown`, `must_use_candidate`, `missing_errors_doc`, `too_many_lines`, casts).
- [ ] **rustfmt** never ran; long lines and import order will be rewritten by `fmt -- --fix`.
- [ ] **`--locked`** fails until step 2 is done.
- [ ] Out of roadmap order: Phase 2 code was written before Phase 1's spikes exited (owner: keep coding).

## 2. Toolchain (1.1) — `docker/build.Dockerfile`, `compose.yaml`
- [ ] `snapshot.debian.org/archive/debian/20261020T000000Z` was never fetched (blocked from the authoring
      sandbox). If apt fails, pick another timestamp.
- [ ] Debian trixie packages `wine` + `wine64`, and headless `wineboot --init`, are untested.
- [ ] `cargo xwin cache xwin` bakes the SDK; cargo-xwin may still write into `/opt/xwin` at run time (made
      world-writable for that).
- [ ] Tool checksums: sccache and cargo-deny match upstream `.sha256` files; **cargo-xwin and nextest have
      no upstream checksum** (hashes are of the files downloaded 2026-10-22).
- [ ] ARM64 hosts (Apple Silicon etc.) untested; T2 Wine is x86_64 only.
- [ ] Containers run as root: on Linux hosts `dist/` and `target/ffi-check/` end up root-owned.
- [ ] `compose.yaml` uses local image tags; pin to GHCR digests after the first `toolchain.yml` run (1.3).

## 3. Workspace & xtask (1.2, 1.3a)
- [ ] Layering table in `xtask/src/layering.rs` is my reading of doc 02 §2 (e.g. `dpro-ui → dpro-win`,
      `dpro-mcp → dpro-broker`, bins may use anything except `dpro-sys`). Confirm.
- [ ] `xtask/data/winpe-dlls.txt` has 4 seed DLLs marked "verify" (kernel32, ntdll, bcryptprimitives,
      api-ms-win-core-synch-l1-2-0). The first `build` may show other std imports.
- [ ] PE reader assumes RVA-based delay-import descriptors (all modern linkers).
- [ ] `i18n-check` treats Fluent continuation lines heuristically (column-0 rule).

## 4. CI (1.3)
- [ ] No workflow has ever run. GHA layer cache (`type=gha`) needs the buildx setup in the composite action.
- [ ] `adk-headers`: winget id `Microsoft.WindowsADK` and the `adksetup` switches
      (`/features OptionId.DeploymentTools /quiet /norestart /ceip off`) are from memory.
- [ ] `release.yml` (draft release, attestations) untested; needs a `v*` tag to exercise.
- [ ] The Windows T3 job is a placeholder until `compose.windows.yaml` lands (S-02, roadmap 1.5).

## 5. FFI bindings (S-03) — `crates/dpro-sys`
- [ ] **Every signature, constant and struct layout is from memory.** `ffi-check` is the gate; nothing in
      `dpro-win` is trustworthy until it passes.
- [ ] DISM structs assume `#pragma pack(1)` (`DismFeature` = 12 bytes packed vs 16 natural).
- [ ] `WIM_MSG = WM_APP + 0x1476`, `WIM_MSG_PROGRESS = WIM_MSG + 1`, the `WIM_*` access/creation values,
      `DISM_MOUNT_*`/`DISM_*_IMAGE` flags, `DismPackageFeatureState` values, `DISM_ONLINE_IMAGE` string.
- [ ] `WIMRegisterMessageCallback` takes `FARPROC` in the header; we pass a typed callback (same ABI).
- [ ] `raw-dylib` linking of `dismapi.dll`/`wimgapi.dll` through cargo-xwin is untested.
- [ ] `ffi-check` relies on clang-cl accepting `__typeof__` + `__builtin_types_compatible_p` in C mode,
      and on finding the xwin splat under `$XWIN_CACHE_DIR`.

## 6. Core & engine (2.1, 2.2) — `dpro-core`, `dpro-engine`
- [ ] Decisions #23 (row storage), #24 (`FileRestore`), #25 (external tagging; variant order is on-disk format).
- [ ] Plan JSON resolves strings lossily (invalid UTF-16 in paths → U+FFFD); TD-0002 must decide.
- [ ] Protected-path list covers TD-0001 §8 names only (no EFI system partition by volume, no
      relocated pagefiles, Windows dir assumed to be `\Windows`).
- [ ] Not implemented yet: crash handler/minidumps (2.1), plan fingerprint (S-04), journal file location +
      ACL, quarantine folder + TTL purge, truncating a torn journal on disk, `xtask perf` scenarios,
      `iai-callgrind` benches.
- [ ] Inverses are captured for a whole batch before any change in it is applied. Correct for
      overwrite-style kinds (property-tested); a future kind whose inverse depends on an earlier change in
      the same batch would need `batch = 1`.
- [ ] Benchmarks exist but no baseline was measured; TD-0001 §7 numbers are still `_tbd_`.

## 7. Windows wrappers (2.3) — `crates/dpro-win`
- [ ] `windows-sys` 0.61.2 signatures, constants and module paths were checked against the crate source
      (crates.io tarball); the **calls themselves have never run**.
- [ ] `dism.rs`/`wim.rs` sit on the unverified S-03 bindings (§5).
- [ ] `RegLoadAppKeyW` needs the hive file not to be in use; untested against a real offline hive.
- [ ] `privilege::enable` relies on `GetLastError() == ERROR_NOT_ALL_ASSIGNED` after a "successful"
      `AdjustTokenPrivileges` (documented behavior, untested).
- [ ] `from_wide_ptr` bounds DISM strings at 32 767 units; DISM documents NUL termination, not a length.
- [ ] Not yet: VSS, BCD, WUA, MSI, ETW, SCM, tasks, firewall wrappers; cancel-event bridging
      (`CancelToken` → manual-reset event for DISM's `CancelEvent`); WIMGAPI message callback.
- [ ] T2 (Wine) runs the pure parts only (registry value encoding, wide strings, long paths).

## 8. Targets & actors (2.4) — `dpro-core::target`, `dpro-engine::{discover, actor}`
- [ ] Offline installs are detected by `<root>\Windows\System32\config\SYSTEM` only (no other Windows
      folder names, no BitLocker-locked volumes, no unassigned volumes / `\\?\Volume{…}` paths).
- [ ] Mounted images aren't discovered yet (needs `DismGetMountedImageInfo`, not bound).
- [ ] WinPE: the "online" target is the WinPE image itself; nothing marks or hides it yet.
- [ ] Actors use `std::sync::mpsc` rather than `crossbeam-channel` (coding standards §5 names crossbeam);
      no engine facade (`Engine::plan/apply/undo`, TD-0001 §5.5) on top of them yet.
- [ ] A panicking request ends the actor thread; callers then get `ActorGone`. Release builds abort on
      panic anyway (`panic = "abort"`).
