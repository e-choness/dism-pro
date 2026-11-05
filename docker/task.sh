#!/usr/bin/env bash
# Entry point for every compose service (ADR-0003): `task.sh <task> [args…]`.
# Runs inside the toolchain container only. Extra args from
# `docker compose run --rm <task> -- <args>` are appended; a leading `--` is dropped.
set -euo pipefail

task="${1:?usage: task.sh <fmt|lint|test|test-win|bench|bench-gate|deny|build|t3-tools|ffi-check|lock|xwin> [args…]}"
shift
if [[ "${1:-}" == "--" ]]; then shift; fi

if [[ ! -f /.dockerenv && -z "${container:-}" ]]; then
  echo "task.sh runs inside the DISM Pro toolchain container; use 'docker compose run --rm ${task}'." >&2
  exit 2
fi

win_targets=(x86_64-pc-windows-msvc aarch64-pc-windows-msvc)

# The MSVC CRT + Windows SDK for the Windows targets. They aren't in the image because they
# aren't redistributable (decision #5): they're downloaded from Microsoft into the xwin volume
# ($XWIN_CACHE_DIR) at the versions the Dockerfile pins, then checked against
# XWIN_SPLAT_SHA256, so every build compiles and links against the same files. A download that
# fails or doesn't match leaves no marker, and the next run starts it again.
xwin_splat() {
  local cache="${XWIN_CACHE_DIR:?}" want="${XWIN_SPLAT_SHA256:?}"
  local marker="${cache}/verified"
  if [[ -f "${marker}" && "$(<"${marker}")" == "${want}" ]]; then return 0; fi
  if [[ "${XWIN_ACCEPT_LICENSE:-}" != 1 ]]; then
    echo "Set XWIN_ACCEPT_LICENSE=1 to accept the Microsoft CRT/SDK license (decisions-log #5)." >&2
    exit 1
  fi
  # One download per volume when several containers start at once; the lock goes with the subshell.
  (
    flock 9
    if [[ -f "${marker}" && "$(<"${marker}")" == "${want}" ]]; then exit 0; fi
    rm -rf "${cache:?}/xwin" "${marker}"
    start=${SECONDS}
    cargo xwin cache xwin
    got=$(cd "${cache}/xwin" && find crt sdk -type f -print0 | LC_ALL=C sort -z \
      | xargs -0 sha256sum | sha256sum)
    got=${got%% *}
    if [[ "${got}" != "${want}" ]]; then
      rm -rf "${cache:?}/xwin"
      msg="the CRT ${XWIN_CRT_VERSION} + SDK ${XWIN_SDK_VERSION} files from Microsoft hash to ${got}, not the pinned ${want}. After a version bump, pin the new hash as XWIN_SPLAT_SHA256 in docker/build.Dockerfile."
      if [[ -n "${GITHUB_ACTIONS:-}" ]]; then echo "::error title=xwin::${msg}"; fi
      echo "xwin: ${msg}" >&2
      exit 1
    fi
    echo "${want}" > "${marker}"
    echo "xwin: CRT ${XWIN_CRT_VERSION} + SDK ${XWIN_SDK_VERSION} downloaded and verified in $((SECONDS - start)) s"
  ) 9> "${cache}/.lock"
}

case "${task}" in
  fmt)
    # `fmt -- --fix` rewrites files instead of checking them.
    if [[ "${1:-}" == "--fix" ]]; then exec cargo fmt --all; fi
    exec cargo fmt --all -- --check "$@"
    ;;
  lint)
    # Native (pure crates, xtask) and Windows targets: dpro-sys/dpro-win are cfg(windows).
    # --all-targets skips `bench = false` targets, so the `ir` benches (Linux only, like
    # valgrind) are named.
    cargo clippy --workspace --all-targets --bench ir --locked -- -D warnings
    xwin_splat
    for t in "${win_targets[@]}"; do
      cargo xwin clippy --workspace --all-targets --locked --target "${t}" -- -D warnings
    done
    cargo xtask layering
    cargo xtask i18n-check
    ;;
  test)
    # nextest doesn't run doctests, so `cargo test --doc` follows it. Arguments go to nextest.
    cargo nextest run --workspace --locked "$@"
    exec cargo test --doc --workspace --locked
    ;;
  test-win)
    # T2: x86_64 only; the Wine runner is set in the t2 image.
    xwin_splat
    exec cargo xwin test --workspace --locked --target x86_64-pc-windows-msvc "$@"
    ;;
  bench)
    # criterion, advisory timings. The `ir` benches aren't run by default (bench = false).
    exec cargo bench --workspace --locked "$@"
    ;;
  bench-gate)
    # Doc 04 §3's deterministic gate: the instruction counts of every crate's `ir` bench (gungraun
    # under Callgrind) against benches/baseline.json. `bench-gate -- --update` rewrites the
    # baseline from this run instead; a PR that does so says why.
    # gungraun turns ASLR off with personality(2), which Docker's default seccomp profile refuses.
    # The counts don't depend on it: Valgrind places the program's memory itself, and repeated
    # runs gave identical counts (TD-0009 §7).
    export GUNGRAUN_ALLOW_ASLR=yes
    summaries=target/gungraun/summaries.jsonl
    mkdir -p "$(dirname "${summaries}")"
    cargo bench --workspace --locked --bench ir -- --output-format=json > "${summaries}"
    exec cargo xtask bench-gate "${summaries}" "$@"
    ;;
  deny)
    exec cargo deny --workspace --locked check "$@"
    ;;
  t3-tools)
    # Dev tools the T3 job runs on the x64 runner VM, never shipped (dist/ stays release-only):
    # regf_dump, the hive reader's side of .github/scripts/regf-check.ps1 (ADR-0008); pipe_t3,
    # the broker pipe probe (roadmap 2.7); wim_t3, the WIMGAPI probe (Wine has no wimgapi.dll,
    # roadmap 2.3); drivers_t3, the DISM driver-call probe (roadmap 2.5); the dpro-win and
    # dpro-broker test binaries, whose
    # crash-dump, pipe and peer tests need real Windows (dbghelp.dll, WER, integrity labels);
    # dismpro-cli's MCP stdio test, which needs dismapi.dll and runs the release CLI (DISMPRO_CLI);
    # and ffi-check-source, which prints the S-03 checks that .github/scripts/adk.ps1 compiles
    # against the ADK headers on the runner, so those headers never leave it (decision #22).
    xwin_splat
    t=x86_64-pc-windows-msvc
    cargo xwin build --locked --release -p dpro-engine --example regf_dump --target "${t}" "$@"
    cargo xwin build --locked --release -p dpro-sys --example ffi-check-source --target "${t}" "$@"
    cargo xwin build --locked --release -p dpro-win --example pipe_t3 --example wim_t3 \
      --example drivers_t3 --target "${t}" "$@"
    # The unit-test binary of package $1: cargo prints "Executable unittests src/lib.rs (<path>)".
    test_exe() {
      cargo xwin test --color never --locked --no-run -p "$1" --target "${t}" "${@:2}" 2>&1 \
        | tee /dev/stderr | sed -n 's/^ *Executable unittests src\/lib\.rs (\(.*\))$/\1/p'
    }
    win_tests=$(test_exe dpro-win "$@")
    broker_tests=$(test_exe dpro-broker "$@")
    mcp_tests=$(cargo xwin test --color never --locked --no-run -p dismpro-cli --test mcp_stdio \
      --target "${t}" "$@" 2>&1 \
      | tee /dev/stderr | sed -n 's/^ *Executable tests\/mcp_stdio\.rs (\(.*\))$/\1/p')
    rm -rf dist-t3
    mkdir -p dist-t3
    cp "target/${t}/release/examples/"{regf_dump,pipe_t3,wim_t3,drivers_t3,ffi-check-source}.exe dist-t3/
    cp "${win_tests:?no dpro-win test binary}" dist-t3/dpro-win-tests.exe
    cp "${broker_tests:?no dpro-broker test binary}" dist-t3/dpro-broker-tests.exe
    cp "${mcp_tests:?no dismpro-cli mcp test binary}" dist-t3/dismpro-cli-mcp-tests.exe
    ;;
  build)
    # Ship binaries only: xtask is a host tool, and dist/ starts empty so no stale exe lingers.
    xwin_splat
    for t in "${win_targets[@]}"; do
      cargo xwin build --workspace --exclude xtask --locked --release --target "${t}" "$@"
      rm -rf "dist/${t}"
      mkdir -p "dist/${t}"
      cp target/"${t}"/release/{dismpro,dismpro-cli}.exe "dist/${t}/"
    done
    size_args=()
    for t in "${win_targets[@]}"; do size_args+=(--target "${t}"); done
    cargo xtask size "${size_args[@]}"
    cargo xtask pe-compat "${size_args[@]}"
    ;;
  lock)
    # Resolves new dependencies into Cargo.lock (keeps existing pins). Commit the result.
    exec cargo update --workspace "$@"
    ;;
  ffi-check)
    # S-03: dpro-sys bindings vs Microsoft headers, for a local run with the ADK headers mounted
    # (DPRO_FFI_INCLUDE). CI runs the same checks on the Windows runner (adk.ps1 -Check).
    xwin_splat
    exec cargo xtask ffi-check "$@"
    ;;
  xwin)
    # Only the CRT/SDK download (CI runs it as its own step, so its time shows per job).
    xwin_splat
    ;;
  *)
    echo "unknown task: ${task}" >&2
    exit 2
    ;;
esac
