#!/usr/bin/env bash
# Screenshots every window and panel of the app, in both themes, for a
# visual review. Run from the repository root after a release build:
#
#     bash scripts/ui-screens.sh [out-dir]
#
# XYLEM_WINDOW_POS=x,y places the windows (for example on a second monitor).
# THEMES="light" limits the run to one theme. Each shot is its own short run
# with a fresh settings folder.
set -u
out="${1:-${TMPDIR:-/tmp}/xylem-screens}"
mkdir -p "$out"
native="$(cygpath -m "$out" 2>/dev/null || echo "$out")"
repo="$(pwd -W 2>/dev/null || pwd)"
bin=./target/release/xylem
[ -x "$bin.exe" ] && bin="$bin.exe"
manual="$repo/samples/manual-sample.xml"
dm="$repo/samples/s1000d/DMC-XYLEM-A-29-20-00-00A-040A-A_001-00_en-US.XML"

shot() { # name, theme, script, [file]
    rm -rf "$out/cfg"
    cp samples/manual-sample.xml "$out/doc.xml"
    XYLEM_WINDOW_SIZE="${SIZE:-}" XYLEM_THEME="$2" XYLEM_CONFIG_DIR="$native/cfg" XYLEM_PACKAGES=samples/doctypes \
        XYLEM_SCRIPT="$3;;wait 12;;shot $native/$1-$2.png;;quit" \
        timeout 90 "$bin" ${4:-"$native/doc.xml"} > /dev/null 2>&1
    echo "$1-$2"
}

for t in ${THEMES:-light dark}; do
    shot welcome "$t" "wait 2" " "
    shot tags-on "$t" "caret Inspect the"
    shot normal "$t" "cmd View normal;;caret Weekly"
    shot source "$t" "cmd View source"
    shot menu-file "$t" "click 19 17"
    shot context-menu "$t" "rclickat 4 4 Inspect the hydraulic"
    shot palette "$t" "caretend Pa).;;key Space ctrl"
    shot find "$t" "cmd Find;;type hydraulic"
    shot find-in-files "$t" "cmd FindInFiles;;pick $repo/samples;;type lift;;key Enter;;wait 30"
    shot preferences "$t" "cmd Preferences"
    shot new-document "$t" "cmd New"
    shot insert-table "$t" "caretend Pa).;;cmd InsertTable"
    shot shortcuts "$t" "cmd Shortcuts"
    shot about "$t" "cmd About"
    shot updates "$t" "cmd CheckUpdates;;wait 20"
    shot comment "$t" "select values;;cmd NewComment;;type Check this value."
    shot review "$t" "cmd TrackChanges;;caretend Check the fluid level;;type , then top up;;select hydraulic;;key Delete;;cmd ReviewPanel"
    shot unsaved "$t" "caretend Inspect the;;type x;;key W ctrl"
    shot error-toast "$t" "cmd CheckBrex;;pick -;;wait 2"
    shot new-s1000d "$t" "cmd NewS1000d"
    shot applicability "$t" "cmd Applicability" "$dm"
    shot dm-info "$t" "cmd S1000dInfo" "$dm"
    shot project "$t" "pick $repo/samples/s1000d;;cmd ShowProject;;cmd ProjectFolder;;wait 20;;cmd CheckReferences;;wait 10" "$dm"
    shot project-empty "$t" "cmd ShowProject"
    SIZE=700,520 shot narrow "$t" "caret Inspect the"
done
echo "Screenshots in $out"
