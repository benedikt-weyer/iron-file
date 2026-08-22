# Command Line Picker

Use picker mode when another program needs the user to choose filesystem
locations:

```sh
iron-file-iced --mode picker --file --single
```

Picker mode remains attached to the caller. When the user confirms, it writes
one selected path per line to standard output and exits with status `0`. A
cancelled picker exits without selected paths.

## Parameters

| Parameter | Meaning |
| --- | --- |
| `PATH` | First positional argument. Opens this file or directory initially. Relative paths are resolved from the invoking directory. When omitted in picker mode, the picker reopens the last directory a picker was closed in (see below) instead of the invoking directory. |
| `-f`, `--follow` | Keep the standard browser attached to the terminal and stream backend logs. |
| `--mode picker` | Run the Iced frontend as a picker. `--picker` is an alias. |
| `--file` | Accept files. This is the picker default. |
| `--folder` | Accept folders instead of files. |
| `--single` | Accept one location. This is the picker default. |
| `--multiple`, `--multi` | Accept multiple locations. |
| `--save-name NAME` | Show an editable save-file name field. Used by the portal `SaveFile` flow. |

```sh
# Open the current directory or a relative location.
iron-file .
iron-file ./Documents

# Pick multiple files starting in Downloads.
iron-file-iced ~/Downloads --mode picker --file --multiple

# Pick a folder.
iron-file-iced --mode picker --folder --single
```

The type and cardinality flags apply only when picker mode is active.
For a folder picker, confirming without an explicit selection chooses the
currently open folder. Double-click a folder to navigate into it; a single
click selects it.

When `--save-name` is present, the picker shows the suggested name in a bottom
input bar. Editing it reveals a reset button that restores the original name.

## Remembering the Last Directory

Every time a picker window closes (confirmed or cancelled), the directory it
was showing is saved to the user's `config.toml` state file (next to the
active-profile setting). The next picker launched without an explicit `PATH`
reopens that directory instead of the invoking directory, so the XDG Desktop
Portal file/folder chooser — which never passes a starting path — resumes
where the user left off. Passing an explicit `PATH` always takes precedence
over the remembered directory.

## Development Scripts

The repository provides shortcuts for the four picker combinations:

| Script | Picker mode |
| --- | --- |
| `pick-file [PATH]` | One file |
| `pick-files [PATH]` | Multiple files |
| `pick-folder [PATH]` | One folder |
| `pick-folders [PATH]` | Multiple folders |

## Save Helpers

The save helpers select one destination folder and print the resulting paths.
They require simple file names, and reject directory separators and traversal
names.

```sh
# Select one destination, edit its name if needed, and print the final path.
save-file report.txt ~/Documents

# Use -- to separate multiple names from an optional initial folder.
save-files first.txt second.txt -- ~/Documents
```
