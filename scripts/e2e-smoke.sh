#!/usr/bin/env bash
# End-to-end smoke tests that drive the real app through XYLEM_SCRIPT.
#
# Click coordinates assume the default 1400x900 window and the Windows UI
# font (Segoe UI); on other platforms they need adjusting. Run from the
# repository root after `cargo build --release -p xylem-app`.
set -u
out="${TMPDIR:-/tmp}/xylem-e2e"
mkdir -p "$out"
# The app needs a native path inside XYLEM_SCRIPT (Git Bash on Windows).
native="$(cygpath -m "$out" 2>/dev/null || echo "$out")"
bin=./target/release/xylem
[ -x "$bin.exe" ] && bin="$bin.exe"
fail=0

run() { # name, script, expected substring in the saved XML
    XYLEM_PACKAGES=samples/doctypes XYLEM_SCRIPT="$2;;save $native/$1.xml;;quit" \
        timeout 90 "$bin" samples/manual-sample.xml > /dev/null 2>&1
    if grep -qF "$3" "$out/$1.xml" 2>/dev/null; then
        echo "ok    $1"
    else
        echo "FAIL  $1"
        fail=1
    fi
}

run typing-and-navigation \
    "click 513 280;;type  Then bleed them.;;key Enter;;type Second step text;;key B ctrl;;type BOLD;;key Home;;key ArrowUp;;key End;;type X;;key Home;;type Y" \
    "YInspect the hydraulic lines Then bleed them.X</para><para>Second step text<b>BOLD</b>.</para>"
run table-dialog \
    "click 780 252;;click 884 33;;wait 20;;click 272 224;;wait 5;;type A1;;key Tab;;type B1" \
    "<entry>A1</entry><entry>B1</entry>"
run surround-palette \
    "dblclick 525 224;;key Space ctrl;;wait 5;;type i;;key Enter;;wait 5" \
    "<b>safe <i>operation</i></b>"
run split-from-menu \
    "click 390 322;;click 139 12;;wait 12;;click 170 172;;wait 5" \
    "<para>Check the</para><para> fluid level.</para>"
run find-next \
    "click 600 130;;key F ctrl;;wait 20;;click 470 115;;type hydraulic;;wait 3;;click 700 790;;key F3;;type ONE" \
    "ONE Lift"
run type-after-closing-tag \
    "click 700 280;;type Z" \
    "lines.Z</para>"
exit $fail
