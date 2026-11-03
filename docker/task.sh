#!/usr/bin/env bash
# Entry point for every compose service (ADR-0003): `task.sh <task> [args…]`.
# Runs inside the toolchain container only. Extra args from
# `docker compose run --rm <task> -- <args>` are appended; a leading `--` is dropped.
set -euo pipefail

task="${1:?usage: task.sh <fmt|lint|test|test-win|bench|deny|build|t3-tools|ffi-check|lock> [args…]}"
shift
if [[ "${1:-}" == "--" ]]; then shift; fi

if [[ ! -f /.dockerenv && -z "${container:-}" ]]; then
  echo "task.sh runs inside the DISM Pro toolchain container; use 'docker compose run --rm ${task}'." >&2
  exit 2
fi

win_targets=(x86_64-pc-windows-msvc aarch64-pc-windows-msvc)

case "${task}" in
  fmt)
    # `fmt -- --fix` rewrites files instead of checking them.
    if [[ "${1:-}" == "--fix" ]]; then exec cargo fmt --all; fi
    exec cargo fmt --all -- --check "$@"
    ;;
  lint)
    # Native (pure crates, xtask) and Windows targets: dpro-sys/dpro-win are cfg(windows).
    cargo clippy --workspace --all-targets --locked -- -D warnings
    for t in "${win_targets[@]}"; do
      cargo xwin clippy --workspace --all-targets --locked --target "${t}" -- -D warnings
    done
    cargo xtask layering
    cargo xtask i18n-check
    ;;
  test)
    exec cargo nextest run --workspace --locked "$@"
    ;;
  test-win)
    # T2: x86_64 only; the Wine runner is set in the t2 image.
    exec cargo xwin test --workspace --locked --target x86_64-pc-windows-msvc "$@"
    ;;
  bench)
    exec cargo bench --workspace --locked "$@"
    ;;
  deny)
    exec cargo deny --workspace --locked check "$@"
    ;;
  t3-tools)
    # Dev tools the T3 job runs on the x64 runner VM, never shipped (dist/ stays release-only):
    # regf_dump, the hive reader's side of .github/scripts/regf-check.ps1 (ADR-0008); pipe_t3,
    # the broker pipe probe (roadmap 2.7); the dpro-win and dpro-broker test binaries, whose
    # crash-dump, pipe and peer tests need real Windows (dbghelp.dll, WER, integrity labels); and
    # dismpro-cli's MCP stdio test, which needs dismapi.dll and runs the release CLI (DISMPRO_CLI).
    t=x86_64-pc-windows-msvc
    cargo xwin build --locked --release -p dpro-engine --example regf_dump --target "${t}" "$@"
    cargo xwin build --locked --release -p dpro-win --example pipe_t3 --target "${t}" "$@"
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
    cp "target/${t}/release/examples/regf_dump.exe" "target/${t}/release/examples/pipe_t3.exe" dist-t3/
    cp "${win_tests:?no dpro-win test binary}" dist-t3/dpro-win-tests.exe
    cp "${broker_tests:?no dpro-broker test binary}" dist-t3/dpro-broker-tests.exe
    cp "${mcp_tests:?no dismpro-cli mcp test binary}" dist-t3/dismpro-cli-mcp-tests.exe
    ;;
  build)
    # Ship binaries only: xtask is a host tool, and dist/ starts empty so no stale exe lingers.
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
    # S-03: dpro-sys bindings vs Microsoft headers. ADK headers via DPRO_FFI_INCLUDE.
    exec cargo xtask ffi-check "$@"
    ;;
  *)
    echo "unknown task: ${task}" >&2
    exit 2
    ;;
esac
