#!/usr/bin/env bash
# End-to-end smoke tests that drive the real app through XYLEM_SCRIPT.
#
# Steps find their targets by text, label or command name rather than by
# screen position (see crates/xylem-app/src/script.rs), so the cases survive
# layout changes. Set XYLEM_WINDOW_POS=x,y to open the test windows somewhere
# out of the way, such as a second monitor. Run from the
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
    "caretend Inspect the hydraulic lines;;type  Then bleed them.;;key Enter;;type Second step text;;key B ctrl;;type BOLD;;key Home;;key ArrowUp;;key End;;type X;;key Home;;type Y" \
    "YInspect the hydraulic lines Then bleed them.X</para><para>Second step text<b>BOLD</b>.</para>"
run table-dialog \
    "caretend Pa).;;cmd InsertTable;;wait 5;;clickui Insert table;;wait 5;;type A1;;key Tab;;type B1" \
    "<entry>A1</entry><entry>B1</entry>"
check table-dialog-keeps-text "$out/table-dialog.xml" "<para>Torque values are given in N·m; pressure in bar (10<sup>5</sup> Pa).</para>"
# Enter typed in the document goes to the document, not to an open dialog.
run enter-goes-to-document \
    "cmd Shortcuts;;wait 5;;caretend Inspect the hydraulic lines;;key Enter;;type NEXT" \
    "hydraulic lines</para><para>NEXT.</para>"
run surround-palette \
    "select operation;;key Space ctrl;;wait 5;;type i;;key Enter;;wait 5" \
    "<b>safe <i>operation</i></b>"
run split-from-menu \
    "caretend Check the;;cmd SplitElement;;wait 5" \
    "<para>Check the</para><para> fluid level.</para>"
run find-next \
    "cmd Find;;wait 5;;type hydraulic;;wait 3;;clickat 2 4 Press the red;;key F3;;type ONE" \
    "ONE Lift"
run type-after-closing-tag \
    "clickat 400 4 Inspect the hydraulic;;type Z" \
    "lines.Z</para>"
run copy-paste-fragment     "caret This manual describes;;key End shift;;key C ctrl;;caretend Check the fluid level;;paste This manual describes the safe operation of the lift. Read Safety before use.;;wait 5"     "fluid levelThis manual describes the <b>safe operation</b> of the lift. Read <xref linkend=\"safety\">Safety</xref>"
run cut-cell-text     "select Daily;;key X ctrl;;wait 3"     "<entry>Hydraulic hoses</entry><entry></entry>"

# Read-only elements: a copy of the sample package with <warning> locked.
rm -rf "$out/ro" && cp -r samples/doctypes "$out/ro"
sed -i 's/^\[behaviour\]$/[behaviour]\nread_only_elements = ["warning"]/' "$out/ro/manual/doctype.toml"
run read-only-element     "caretend Never stand under;;type ZZ;;key BackSpace;;key BackSpace;;key BackSpace;;wait 3"     "<warning><para>Never stand under a raised load.</para></warning>" "$native/ro"

# File > Import XMAX customization, then open a document that uses it.
rm -rf "$out/pk" "$out/cfg" && mkdir -p "$out/pk"
XYLEM_CONFIG_DIR="$native/cfg" XYLEM_PACKAGES="$native/pk"     XYLEM_SCRIPT="pick $repo/samples/xmax/Memo/Memo.ctm;;cmd ImportXmax;;wait 20;;quit"     timeout 90 "$bin" samples/manual-sample.xml > /dev/null 2>&1
check import-from-menu "$out/pk/Memo/doctype.toml" 'schema = "Memo.xsd"'

# Ctrl+click on a link to another file opens it at the #id.
mkdir -p "$out/links"
printf '<doc><p>See <link href="b.xml#second">the second part</link>.</p></doc>
' > "$out/links/a.xml"
printf '<doc><p id="first">First.</p><p id="second">Second.</p></doc>
' > "$out/links/b.xml"
XYLEM_CONFIG_DIR="$native/cfg" XYLEM_SCRIPT="ctrlclickat 4 6 the second part;;wait 10;;type X;;save $native/link.xml;;quit"     timeout 90 "$bin" "$native/links/a.xml" > /dev/null 2>&1
check follow-file-link "$out/link.xml" '<p id="second">XSecond.</p>'

# Edit > Find in files: replace across a folder. The open document is
# clean, so it is rewritten on disk and reloaded; the other file is
# rewritten without being opened, and its id attribute is left alone.
rm -rf "$out/ff" "$out/cfg-ff" && mkdir -p "$out/ff"
printf '<doc><p>The pump is primed.</p></doc>\n' > "$out/ff/a.xml"
printf '<doc><p id="pump">Check the pump &amp; seals.</p></doc>\n' > "$out/ff/b.xml"
XYLEM_CONFIG_DIR="$native/cfg-ff" XYLEM_SCRIPT="key F ctrl shift;;wait 5;;type pump;;key Enter;;wait 30;;clickui Replace with;;type valve;;clickui Replace all…;;wait 20;;clickui Replace;;wait 30;;save $native/ff-open.xml;;quit" \
    timeout 90 "$bin" "$native/ff/a.xml" > /dev/null 2>&1
check find-in-files-reloads-open "$out/ff-open.xml" "<p>The valve is primed.</p>"
check find-in-files-writes-closed "$out/ff/b.xml" '<doc><p id="pump">Check the valve &amp; seals.</p></doc>'

# Project tab: scan the sample folder, put the caret in the publication
# module's entry title and insert a reference to the lift cylinder module.
# Where it lands depends on whether the S1000D schemas are installed (with
# them it goes after the title), so only the reference itself is checked.
rm -rf "$out/cfg-project"
XYLEM_CONFIG_DIR="$native/cfg-project" XYLEM_SCRIPT="pick $repo/samples/s1000d;;cmd ShowProject;;wait 5;;cmd ProjectFolder;;wait 30;;caretend Hydraulic system;;wait 5;;clickui DMC-XYLEM-A-29-20-00-00A-040A-A;;wait 10;;clickui Insert reference;;wait 10;;save $native/pm.xml;;quit" \
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
    XYLEM_SCRIPT="key T ctrl shift;;caretend Inspect the hydraulic lines;;type  Then bleed them.;;key Enter;;type More;;save $native/tracked-typing.xml;;quit" \
    timeout 90 "$bin" samples/manual-sample.xml > /dev/null 2>&1
check tracked-typing-start "$out/tracked-typing.xml" 'lines<?xm-insertion_mark_start author="'
# Enter inside your own insertion makes the break part of it.
check tracked-typing-end "$out/tracked-typing.xml" ' Then bleed them.</para>'
check tracked-after-enter "$out/tracked-typing.xml" 'More<?xm-insertion_mark_end?>.</para>'
starts=$(grep -o 'xm-insertion_mark_start' "$out/tracked-typing.xml" | wc -l)
ends=$(grep -o 'xm-insertion_mark_end' "$out/tracked-typing.xml" | wc -l)
if [ "$starts" -eq 1 ] && [ "$ends" -eq 1 ]; then echo "ok    tracked-marks-paired"; else echo "FAIL  tracked-marks-paired"; fail=1; fi
XYLEM_CONFIG_DIR="$native/cfg3" XYLEM_PACKAGES=samples/doctypes \
    XYLEM_SCRIPT="key T ctrl shift;;caretend Inspect the hydraulic lines;;key Backspace;;key Backspace;;key Backspace;;key Backspace;;key Backspace;;type hoses;;save $native/tracked-mid.xml;;key A ctrl shift;;wait 3;;save $native/accept-all.xml;;quit" \
    timeout 90 "$bin" samples/manual-sample.xml > /dev/null 2>&1
check tracked-deletion "$out/tracked-mid.xml" 'data="lines"?><?xm-insertion_mark_start author="'
check tracked-replacement "$out/tracked-mid.xml" 'hoses<?xm-insertion_mark_end?>.</para>'
check accept-all "$out/accept-all.xml" '<para>Inspect the hydraulic hoses.</para>'
# Splitting, joining and moving are tracked: Reject all gives back the file
# byte for byte.
same() { # name, file
    if cmp -s "$2" samples/manual-sample.xml; then echo "ok    $1"; else echo "FAIL  $1"; fail=1; fi
}
for case in "split|caret lines.;;key Enter" "join|caret Torque values;;key Backspace" "move|caret Pump seals;;cmd TableRowUp"; do
    name=${case%%|*}
    XYLEM_CONFIG_DIR="$native/cfg3" XYLEM_PACKAGES=samples/doctypes         XYLEM_SCRIPT="key T ctrl shift;;${case#*|};;save $native/tracked-$name.xml;;cmd ResolveAll reject;;wait 3;;save $native/rejected-$name.xml;;quit"         timeout 90 "$bin" samples/manual-sample.xml > /dev/null 2>&1
    check "tracked-$name" "$out/tracked-$name.xml" '<?xm-insertion_mark_start author="'
    same "reject-$name" "$out/rejected-$name.xml"
done
# Saving keeps the previous version, and the lock file goes when the
# document is closed.
rm -rf "$out/safe" && mkdir -p "$out/safe/docs" && cp samples/manual-sample.xml "$out/safe/docs/s.xml"
XYLEM_CONFIG_DIR="$native/safe/cfg" XYLEM_PACKAGES=samples/doctypes     XYLEM_SCRIPT="caretend Inspect the hydraulic lines;;type SAVED;;key S ctrl;;wait 5;;quit"     timeout 90 "$bin" "$native/safe/docs/s.xml" > /dev/null 2>&1
check save-writes-file "$out/safe/docs/s.xml" "hydraulic linesSAVED"
check save-keeps-backup "$(ls "$out"/safe/cfg/backups/*/s.*.xml 2>/dev/null | head -1)" "Inspect the hydraulic lines.</para>"
if ls -a "$out/safe/docs" | grep -q lock; then echo "FAIL  lock-removed-on-close"; fail=1; else echo "ok    lock-removed-on-close"; fi
exit $fail
