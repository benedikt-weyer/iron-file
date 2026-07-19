# iron-file

`iron-file` is a three-process Rust workspace. The frontend processes use gRPC
over a local Unix-domain socket to browse files through one shared backend.

| Package | Process | Purpose |
| --- | --- | --- |
| `iron-file-backend` | backend | Singleton filesystem service and gRPC server |
| `iron-file-iced` | Iced frontend | Default file-browser GUI client |
| `iron-file-gtk` | GTK4 frontend | GTK4 file-browser GUI client |

## Run

Enter the Nix shell with `direnv allow` or `nix develop`, then start the
backend once:

```sh
run-backend
```

Start any number of frontends in separate terminals. They all connect to the
same backend:

```sh
run
run-gtk
```

The backend listens at `$XDG_RUNTIME_DIR/iron-file-backend.sock`, or at the
path set by `IRON_FILE_SOCKET`. A second backend refuses to start while the
socket is owned by a running backend. If the prior process exited unexpectedly,
the next backend removes the stale socket before binding. An OS-level lock file
also prevents concurrent startup races from producing more than one backend.

Both frontends provide an address bar, parent-folder navigation, clickable
directory/file entries, and text-file previews. Binary files and files larger
than 1 MB display a short preview notice.
