#!/usr/bin/env bash
# Entry point for every compose service (ADR-0003): `task.sh <task> [args…]`.
# Runs inside the toolchain container only. Extra args from
# `docker compose run --rm <task> -- <args>` are appended; a leading `--` is dropped.
set -euo pipefail

task="${1:?usage: task.sh <fmt|lint|test|test-win|bench|deny|build|ffi-check|lock> [args…]}"
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
  build)
    shopt -s nullglob
    for t in "${win_targets[@]}"; do
      cargo xwin build --workspace --locked --release --target "${t}" "$@"
      mkdir -p "dist/${t}"
      exes=(target/"${t}"/release/*.exe)
      if (( ${#exes[@]} )); then cp "${exes[@]}" "dist/${t}/"; fi
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
