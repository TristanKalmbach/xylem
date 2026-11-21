# Team setup

Xylem is a single executable with no runtime dependencies. For a team, the
usual setup is a shared folder (a network share, or a repository everyone
checks out) holding:

```
team/
  xylem-team.toml
  doctypes/          converted XMAX customizations and other packages
  schemas/s1000d/    optional: S1000D schemas, one folder per issue
  dictionaries/      optional: Hunspell .aff/.dic files
  updates/           optional: published builds, see "Updating"
```

## xylem-team.toml

Xylem looks for the file in this order and uses the first it finds:

1. the path in the `XYLEM_TEAM_CONFIG` environment variable;
2. `xylem-team.toml` next to the executable (`xylem --install --team` writes
   one there);
3. `xylem-team.toml` in the user's config folder (`%APPDATA%\Xylem` on
   Windows, `~/.config/Xylem` on Linux, `~/Library/Application Support/Xylem`
   on macOS).

Every key is optional. Relative paths are relative to the file.

```toml
package_dirs = ["doctypes"]          # scanned before the user's own folders
s1000d_schemas = "schemas/s1000d"    # laid out like www.s1000d.org
catalog = "schemas/catalog.xml"      # OASIS catalog or catalog.toml
brex = "brex/DMC-ACME-A-00-00-00-00A-022A-D_001-00_EN-US.XML"
dictionary_dir = "dictionaries"
dictionary_lang = "en_US"
update_source = "updates"
```

Don't copy the shared file elsewhere: its relative paths would then point at
folders that are not there. Use a one-line file that includes it instead:

```toml
include = "//fileserver/xylem/xylem-team.toml"
```

A user's own Preferences win over single values (schema folder, BREX,
dictionary), so someone can try a different BREX without touching the shared
file. Team package folders are scanned before the user's own. When several
packages match a document equally well, the first one found wins: the team's,
and within a folder the one whose folder name sorts first. Give a package a
higher `priority` in its `doctype.toml` to prefer it.

Unknown keys are reported rather than rejected, so older installations keep
working when the shared file gains new settings. Problems with the file show
up as a message at startup and in File › Preferences, and a file that cannot
be read is skipped instead of stopping the editor.

## Installing

```
xylem --install --team //fileserver/xylem/xylem-team.toml
```

On Windows, `xylem.exe` is a GUI program, so PowerShell does not wait for
it and its messages can appear after the next prompt. Add `| Out-Host` (or
run it from `cmd`) to wait for it and see the result in order.

This copies the running executable to a per-user folder and needs no
administrator rights:

| Platform | Program | Also adds |
|----------|---------|-----------|
| Windows | `%LOCALAPPDATA%\Programs\Xylem\xylem.exe` | Start Menu shortcut, entry in Settings › Apps › Installed apps |
| Linux | `~/.local/bin/xylem` | `~/.local/share/applications/xylem.desktop` |
| macOS | `~/Applications/Xylem/xylem` | nothing |

`--team` is optional; with it, a pointer file (`include = ...`) is written
next to the installed program. `xylem --uninstall` (or Uninstall in Installed
apps) removes everything again. Set `XYLEM_INSTALL_DIR` to install somewhere
else; the shortcut and Installed apps entry are then skipped.

## Updating

Pick a folder everyone can read and set it as `update_source` in the team
file. Whoever builds a new version publishes it there:

```
xylem --publish-update //fileserver/xylem/updates
```

That copies the executable and writes `version.txt` (version and size).
Installations check the folder at startup and say when a newer version is
there. Help › Check for updates installs it, and it takes effect on the next
start. `xylem --update` does the same without opening the editor, for
scripts. An older version in the folder is never installed over a newer one.

Bump `version` under `[workspace.package]` in the top-level `Cargo.toml` before building a release,
since that is what the check compares. Each platform needs its own update
folder, because the executables differ.
