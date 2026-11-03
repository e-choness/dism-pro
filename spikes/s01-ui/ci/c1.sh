#!/usr/bin/env bash
# Spike S-01 (roadmap 1.6, ADR-0002), C1: Docker on ubuntu (the T2 image, for Wine).
#
# Lints and tests the spike, builds the three Windows exes (bare, A, B) and B for Linux, then
# measures the C1 gates: exact release sizes, imported DLLs (the WinPE job resolves them against the
# image), and the headless pixel snapshots and 100k-list fling for both options (B natively, A's
# Windows build under Wine: DirectWrite is its text path). Writes out/ (exes and raw results) for
# the Windows jobs, and annotations for the run.
#
#   docker compose run --rm --entrypoint bash test-win spikes/s01-ui/ci/c1.sh
set -euo pipefail
cd "$(dirname "$0")/.."
export CARGO_TARGET_DIR=/src/target/s01 CARGO_TERM_COLOR=never
out=out
rm -rf "$out"
mkdir -p "$out/bin" "$out/c1"

# One annotation from a file: GitHub reads workflow commands from the container's stdout too.
notice() {
    local text
    text=$(sed -e 's/%/%25/g' "$2" | awk 'BEGIN { ORS = "%0A" } { print }')
    echo "::notice title=$1::$text"
}

echo "::group::lint + test"
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo xwin clippy --release --workspace --target x86_64-pc-windows-msvc -- -D warnings
cargo test --workspace
echo "::endgroup::"

echo "::group::build"
cargo build --release -p b
cargo xwin build --release --workspace --target x86_64-pc-windows-msvc
win="$CARGO_TARGET_DIR/x86_64-pc-windows-msvc/release"
cp "$win/bare.exe" "$win/s01-a.exe" "$win/s01-b.exe" "$out/bin/"
echo "::endgroup::"
# The same exes size-optimized (fat LTO, one codegen unit, opt-level s): the best case for the
# size gate, so a verdict doesn't hinge on the profile.
echo "::group::build (size-optimized)"
min=/src/target/s01-min
CARGO_TARGET_DIR=$min CARGO_PROFILE_RELEASE_LTO=fat CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 \
    CARGO_PROFILE_RELEASE_OPT_LEVEL=s cargo xwin build --release --workspace --target x86_64-pc-windows-msvc
echo "::endgroup::"

# Release binary size (C1 exact): the x64 exes, each against the bare Win32 window. Gate ≤ 3 MiB.
sizes() { # directory
    local bare bytes
    bare=$(stat -c %s "$1/bare.exe")
    for exe in bare s01-a s01-b; do
        bytes=$(stat -c %s "$1/$exe.exe")
        awk -v e="$exe" -v s="$bytes" -v b="$bare" 'BEGIN {
            d = s - b
            verdict = e == "bare" ? "" : (d <= 3 * 1048576 ? "  PASS" : "  FAIL")
            printf "%-6s %9d bytes (%5.2f MiB); delta vs bare %+5.2f MiB%s\n", e, s, s / 1048576, d / 1048576, verdict
        }'
    done
}
{
    echo "release profile (the product's):"
    sizes "$out/bin"
    echo "size-optimized (lto fat, codegen-units 1, opt-level s):"
    sizes "$min/x86_64-pc-windows-msvc/release"
} > "$out/c1/size.txt"

# Where the bytes go: each release exe relinked with lld-link's map (only the final crate rebuilds;
# the copies in out/bin are already taken), summed per crate by ci/crate-sizes.awk.
echo "::group::size by crate"
for o in a b; do
    map="$CARGO_TARGET_DIR/s01-$o.lldmap"
    cargo xwin rustc --release -p "$o" --bin "s01-$o" --target x86_64-pc-windows-msvc -- \
        -C "link-arg=/lldmap:$map"
    echo "s01-$o.exe, release profile, file bytes by origin:"
    awk -v top=14 -f ci/crate-sizes.awk "$map"
done > "$out/c1/crates.txt"
cat "$out/c1/crates.txt"
echo "::endgroup::"

# Imported DLLs. pe-compat fails on them (the allowlist is the CLI's), so its exit status is not
# the verdict here: the WinPE job resolves every import against base WinPE itself.
(cd /src && env -u CARGO_TARGET_DIR cargo xtask pe-compat --exe \
    "spikes/s01-ui/$out/bin/bare.exe" "spikes/s01-ui/$out/bin/s01-a.exe" "spikes/s01-ui/$out/bin/s01-b.exe") \
    > "$out/c1/imports.txt" 2>&1 || true
if [ "$(grep -c 'all imports:' "$out/c1/imports.txt")" -ne 3 ]; then
    cat "$out/c1/imports.txt"
    exit 1
fi

# Headless gates. Each run's status is recorded, not fatal: the annotations carry the verdicts.
status=0
headless() { # name, command…
    local name=$1
    shift
    echo "::group::$name"
    "$@" > "$out/c1/$name.txt" 2>&1 || {
        local rc=$?
        echo "$name: exit status $rc" >> "$out/c1/$name.txt"
        status=1
    }
    cat "$out/c1/$name.txt"
    echo "::endgroup::"
}
b="$CARGO_TARGET_DIR/release/s01-b"
# A runs from the target volume: Wine can't map an exe from a Docker Desktop bind mount.
headless snapshot-b "$b" --snapshot "$out/c1/snap-b" --scales 1,1.5
headless snapshot-a wine "$win/s01-a.exe" --snapshot "$out/c1/snap-a" --scales 1,1.5
headless bench-b "$b" --bench --scales 1,1.5 --frames "$out/c1/frames-b.tsv"
headless bench-a wine "$win/s01-a.exe" --bench --scales 1,1.5 --frames "$out/c1/frames-a.tsv"

{
    echo "release binary size, x86_64-pc-windows-msvc (gate: delta <= 3 MiB)"
    cat "$out/c1/size.txt"
    echo
    echo "pixel snapshots (3 screens x 2 themes x 2 scales each)"
    for o in a b; do
        awk -v o="$o" '/^snapshot / { n++; split($3, c, "/"); if (c[1] == c[2]) ok++ }
            /FAIL|exit status/ { bad = bad "\n  " $0 }
            END { printf "%s: %d/%d screens pass every check%s\n", o, ok, n, bad }' "$out/c1/snapshot-$o.txt"
    done
    echo
    echo "100k-list fling, software path (gate: p99 <= 16.6 ms; A under Wine, B native)"
    grep -h '^bench ' "$out/c1/bench-a.txt" "$out/c1/bench-b.txt"
    grep -h 'exit status' "$out/c1/bench-a.txt" "$out/c1/bench-b.txt" || true
} > "$out/c1/summary.txt"
cat "$out/c1/summary.txt"
notice "S-01 C1 gates" "$out/c1/summary.txt"
awk '/^pe-compat: (ok|FAIL)/ { exe = $3; sub(/.*\//, "", exe) }
    /all imports:/ { sub(/^ *all imports: /, ""); print exe ": " $0 }' "$out/c1/imports.txt" > "$out/c1/imports-short.txt"
notice "S-01 imports" "$out/c1/imports-short.txt"
notice "S-01 size by crate" "$out/c1/crates.txt"
exit $status
