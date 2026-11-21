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

repo="$(pwd -W 2>/dev/null || pwd)"

check() { # name, file, expected substring
    if grep -qF "$3" "$2" 2>/dev/null; then
        echo "ok    $1"
    else
        echo "FAIL  $1"
        fail=1
    fi
}

run() { # name, script, expected substring in the saved XML [, package folder]
    XYLEM_PACKAGES="${4:-samples/doctypes}" XYLEM_SCRIPT="$2;;save $native/$1.xml;;quit" \
        timeout 90 "$bin" samples/manual-sample.xml > /dev/null 2>&1
    check "$1" "$out/$1.xml" "$3"
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
run copy-paste-fragment     "click 277 224;;key End shift;;key C ctrl;;click 455 321;;paste This manual describes the safe operation of the lift. Read Safety before use.;;wait 5"     "fluid levelThis manual describes the <b>safe operation</b> of the lift. Read <xref linkend=\"safety\">Safety</xref>"
run cut-cell-text     "dblclick 738 648;;key X ctrl;;wait 3"     "<entry>Hydraulic hoses</entry><entry></entry>"

# Read-only elements: a copy of the sample package with <warning> locked.
rm -rf "$out/ro" && cp -r samples/doctypes "$out/ro"
sed -i 's/^\[behaviour\]$/[behaviour]\nread_only_elements = ["warning"]/' "$out/ro/manual/doctype.toml"
run read-only-element     "click 400 523;;type ZZ;;key BackSpace;;key BackSpace;;key BackSpace;;wait 3"     "<warning><para>Never stand under a raised load.</para></warning>" "$native/ro"

# File > Import XMAX customization, then open a document that uses it.
rm -rf "$out/pk" "$out/cfg" && mkdir -p "$out/pk"
XYLEM_CONFIG_DIR="$native/cfg" XYLEM_PACKAGES="$native/pk"     XYLEM_SCRIPT="pick $repo/samples/xmax/Memo/Memo.ctm;;click 19 12;;wait 10;;click 100 202;;wait 20;;quit"     timeout 90 "$bin" samples/manual-sample.xml > /dev/null 2>&1
check import-from-menu "$out/pk/Memo/doctype.toml" 'schema = "Memo.xsd"'

# Ctrl+click on a link to another file opens it at the #id.
mkdir -p "$out/links"
printf '<doc><p>See <link href="b.xml#second">the second part</link>.</p></doc>
' > "$out/links/a.xml"
printf '<doc><p id="first">First.</p><p id="second">Second.</p></doc>
' > "$out/links/b.xml"
XYLEM_CONFIG_DIR="$native/cfg" XYLEM_SCRIPT="ctrlclick 400 105;;wait 10;;type X;;save $native/link.xml;;quit"     timeout 90 "$bin" "$native/links/a.xml" > /dev/null 2>&1
check follow-file-link "$out/link.xml" '<p id="second">XSecond.</p>'

# Zoom and the last session survive a restart.
rm -rf "$out/cfg2"
XYLEM_CONFIG_DIR="$native/cfg2" XYLEM_PACKAGES=samples/doctypes XYLEM_SCRIPT="key Plus ctrl;;quit"     timeout 90 "$bin" "$repo/samples/manual-sample.xml" > /dev/null 2>&1
XYLEM_CONFIG_DIR="$native/cfg2" XYLEM_PACKAGES=samples/doctypes XYLEM_SCRIPT="save $native/restored.xml;;quit"     timeout 90 "$bin" > /dev/null 2>&1
check session-restore "$out/restored.xml" "Hydraulic Lift Operator Manual"
check zoom-persists "$out/cfg2/app.ron" "zoom:1.1"
exit $fail
