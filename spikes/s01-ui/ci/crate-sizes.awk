# Bytes per crate in a Windows exe, from lld-link's map (/lldmap): the input sections of every
# output section but .bss (which takes no file space), summed by where they came from: a Rust
# crate's rlib, the exe's own crate, or an import or CRT library. Padding, headers and the import
# tables aren't input sections, so the total is a little under the file size.
#
#   awk -v top=12 -f crate-sizes.awk s01-b.lldmap
#
# POSIX awk (the T2 image has mawk): no strtonum, no asort.

function hex(s,    i, n, d) {
    n = 0
    s = tolower(s)
    for (i = 1; i <= length(s); i++) {
        d = index("0123456789abcdef", substr(s, i, 1)) - 1
        n = n * 16 + d
    }
    return n
}

# An rlib's object is listed by its member name, <crate>-<hash>.<crate>.<hash>-cgu.N.rcgu.o; the
# exe's own crate by its path, .../<crate>.<crate>.<hash>-cgu.N.rcgu.o; the static CRT's objects by
# the Windows path they were built at.
function origin(path,    base) {
    base = path
    sub(/^ +/, "", base)
    sub(/:\(.*$/, "", base)                      # the section, after the object
    if (base ~ /\\/) return "(MSVC CRT)"
    if (match(base, /[A-Za-z0-9_.-]+\.lib\(/)) return "(" substr(base, RSTART, RLENGTH - 1) ")"
    sub(/^.*\//, "", base)
    if (base ~ /\.rcgu\.o$/) {
        sub(/\..*$/, "", base)
        sub(/-[0-9a-f]+$/, "", base)
        return base
    }
    return "(" base ")"
}

NR == 1 { next }
{
    if (!match($0, /^[0-9a-f]+ +[0-9a-f]+ +[0-9]+ /)) next
    rest = substr($0, RLENGTH + 1)
    indent = match(rest, /[^ ]/) - 1
    split($0, f, / +/)
    if (indent == 0) { out = rest; next }           # an output section
    if (indent != 8 || out == ".bss") next          # 8: an input section; 16: a symbol
    bytes[origin(rest)] += hex(f[2])
}

END {
    total = 0
    for (k in bytes) total += bytes[k]
    shown = 0
    while (shown < top) {
        best = ""
        for (k in bytes) if (!(k in done) && (best == "" || bytes[k] > bytes[best])) best = k
        if (best == "") break
        done[best] = 1
        printf "  %-28s %9.2f KiB  %5.1f %%\n", best, bytes[best] / 1024, 100 * bytes[best] / total
        shown++
    }
    rest = 0
    for (k in bytes) if (!(k in done)) { rest += bytes[k]; n++ }
    if (n) printf "  %-28s %9.2f KiB  %5.1f %%\n", "(" n " more)", rest / 1024, 100 * rest / total
    printf "  %-28s %9.2f KiB\n", "total in sections", total / 1024
}
