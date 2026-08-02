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
| `PATH` | First positional argument. Opens this file or directory initially. Relative paths are resolved from the invoking directory. |
| `-f`, `--follow` | Keep the standard browser attached to the terminal and stream backend logs. |
| `--mode picker` | Run the Iced frontend as a picker. `--picker` is an alias. |
| `--file` | Accept files. This is the picker default. |
| `--folder` | Accept folders instead of files. |
| `--single` | Accept one location. This is the picker default. |
| `--multiple`, `--multi` | Accept multiple locations. |

```sh
# Pick multiple files starting in Downloads.
iron-file-iced ~/Downloads --mode picker --file --multiple

# Pick a folder.
iron-file-iced --mode picker --folder --single
```

The type and cardinality flags apply only when picker mode is active.
