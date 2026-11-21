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
XYLEM_CONFIG_DIR="$native/cfg" XYLEM_PACKAGES="$native/pk"     XYLEM_SCRIPT="pick $repo/samples/xmax/Memo/Memo.ctm;;click 19 12;;wait 10;;click 100 253;;wait 20;;quit"     timeout 90 "$bin" samples/manual-sample.xml > /dev/null 2>&1
check import-from-menu "$out/pk/Memo/doctype.toml" 'schema = "Memo.xsd"'

# Ctrl+click on a link to another file opens it at the #id.
mkdir -p "$out/links"
printf '<doc><p>See <link href="b.xml#second">the second part</link>.</p></doc>
' > "$out/links/a.xml"
printf '<doc><p id="first">First.</p><p id="second">Second.</p></doc>
' > "$out/links/b.xml"
XYLEM_CONFIG_DIR="$native/cfg" XYLEM_SCRIPT="ctrlclick 400 105;;wait 10;;type X;;save $native/link.xml;;quit"     timeout 90 "$bin" "$native/links/a.xml" > /dev/null 2>&1
check follow-file-link "$out/link.xml" '<p id="second">XSecond.</p>'

# Edit > Find in files: replace across a folder. The open document is
# clean, so it is rewritten on disk and reloaded; the other file is
# rewritten without being opened, and its id attribute is left alone.
rm -rf "$out/ff" "$out/cfg-ff" && mkdir -p "$out/ff"
printf '<doc><p>The pump is primed.</p></doc>\n' > "$out/ff/a.xml"
printf '<doc><p id="pump">Check the pump &amp; seals.</p></doc>\n' > "$out/ff/b.xml"
XYLEM_CONFIG_DIR="$native/cfg-ff" XYLEM_SCRIPT="key F ctrl shift;;wait 5;;type pump;;key Enter;;wait 30;;click 770 189;;type valve;;click 521 282;;wait 20;;click 549 489;;wait 30;;save $native/ff-open.xml;;quit" \
    timeout 90 "$bin" "$native/ff/a.xml" > /dev/null 2>&1
check find-in-files-reloads-open "$out/ff-open.xml" "<p>The valve is primed.</p>"
check find-in-files-writes-closed "$out/ff/b.xml" '<doc><p id="pump">Check the valve &amp; seals.</p></doc>'

# Project tab: scan the sample folder, put the caret in the publication
# module's entry title and insert a reference to the lift cylinder module.
# Where it lands depends on whether the S1000D schemas are installed (with
# them it goes after the title), so only the reference itself is checked.
rm -rf "$out/cfg-project"
XYLEM_CONFIG_DIR="$native/cfg-project" XYLEM_SCRIPT="pick $repo/samples/s1000d;;click 232 12;;wait 5;;click 239 286;;wait 10;;click 33 85;;wait 30;;click 600 381;;wait 10;;click 100 302;;wait 10;;click 103 151;;wait 10;;save $native/pm.xml;;quit" \
    timeout 90 "$bin" "$repo/samples/s1000d/PMC-XYLEM-00000-00001-00_001-00_en-US.XML" > /dev/null 2>&1
check project-insert-reference "$out/pm.xml" '<dmRef><dmRefIdent><dmCode modelIdentCode="XYLEM" systemDiffCode="A" systemCode="29" subSystemCode="2"'

# Zoom and the last session survive a restart.
rm -rf "$out/cfg2"
XYLEM_CONFIG_DIR="$native/cfg2" XYLEM_PACKAGES=samples/doctypes XYLEM_SCRIPT="key Plus ctrl;;quit"     timeout 90 "$bin" "$repo/samples/manual-sample.xml" > /dev/null 2>&1
XYLEM_CONFIG_DIR="$native/cfg2" XYLEM_PACKAGES=samples/doctypes XYLEM_SCRIPT="save $native/restored.xml;;quit"     timeout 90 "$bin" > /dev/null 2>&1
check session-restore "$out/restored.xml" "Hydraulic Lift Operator Manual"
check zoom-persists "$out/cfg2/app.ron" "zoom:1.1"

# Track changes (Ctrl+Shift+T): typing becomes one insertion, Backspace a
# deletion, and Accept all (Ctrl+Shift+A) turns them into plain edits. The
# author is the login name, so only the parts after it are checked.
rm -rf "$out/cfg3"
XYLEM_CONFIG_DIR="$native/cfg3" XYLEM_PACKAGES=samples/doctypes \
    XYLEM_SCRIPT="key T ctrl shift;;click 513 280;;type  Then bleed them.;;key Enter;;type More;;save $native/tracked-typing.xml;;quit" \
    timeout 90 "$bin" samples/manual-sample.xml > /dev/null 2>&1
check tracked-typing-start "$out/tracked-typing.xml" 'lines<?xm-insertion_mark_start author="'
check tracked-typing-end "$out/tracked-typing.xml" ' Then bleed them.<?xm-insertion_mark_end?></para>'
check tracked-after-enter "$out/tracked-typing.xml" 'More<?xm-insertion_mark_end?>.</para>'
starts=$(grep -o 'xm-insertion_mark_start' "$out/tracked-typing.xml" | wc -l)
ends=$(grep -o 'xm-insertion_mark_end' "$out/tracked-typing.xml" | wc -l)
if [ "$starts" -eq 2 ] && [ "$ends" -eq 2 ]; then echo "ok    tracked-marks-paired"; else echo "FAIL  tracked-marks-paired"; fail=1; fi
XYLEM_CONFIG_DIR="$native/cfg3" XYLEM_PACKAGES=samples/doctypes \
    XYLEM_SCRIPT="key T ctrl shift;;click 513 280;;key Backspace;;key Backspace;;key Backspace;;key Backspace;;key Backspace;;type hoses;;save $native/tracked-mid.xml;;key A ctrl shift;;wait 3;;save $native/accept-all.xml;;quit" \
    timeout 90 "$bin" samples/manual-sample.xml > /dev/null 2>&1
check tracked-deletion "$out/tracked-mid.xml" 'data="lines"?><?xm-insertion_mark_start author="'
check tracked-replacement "$out/tracked-mid.xml" 'hoses<?xm-insertion_mark_end?>.</para>'
check accept-all "$out/accept-all.xml" '<para>Inspect the hydraulic hoses.</para>'
exit $fail
