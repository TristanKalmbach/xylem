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
```

## xylem-team.toml

Xylem looks for the file in this order and uses the first it finds:

1. the path in the `XYLEM_TEAM_CONFIG` environment variable;
2. `xylem-team.toml` next to `xylem.exe` (convenient when IT installs it);
3. `xylem-team.toml` in the user's config folder (`%APPDATA%\Xylem` on
   Windows, `~/.config/xylem` on Linux, `~/Library/Application Support/xylem`
   on macOS).

Every key is optional. Relative paths are relative to the file.

```toml
package_dirs = ["doctypes"]          # scanned before the user's own folders
s1000d_schemas = "schemas/s1000d"    # laid out like www.s1000d.org
catalog = "schemas/catalog.xml"      # OASIS catalog or catalog.toml
brex = "brex/DMC-ACME-A-00-00-00-00A-022A-D_001-00_EN-US.XML"
dictionary_dir = "dictionaries"
dictionary_lang = "en_US"
update_source = "//fileserver/tools/xylem"
```

A user's own Preferences win over the team values, so someone can still try
a different schema folder or BREX without touching the shared file. Unknown
keys are reported in the log rather than silently ignored, and a broken file
is skipped instead of stopping the editor.
