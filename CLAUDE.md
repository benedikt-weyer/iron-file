# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`iron-file` is a Rust workspace implementing a Linux file manager as a set of
cooperating processes: a singleton filesystem backend, two interchangeable GUI
frontends, and an XDG Desktop Portal file-chooser backend. GUI clients talk to
the one shared backend over gRPC through a local Unix-domain socket, so
multiple frontend windows/processes see a consistent view and only one process
touches the filesystem.

| Crate | Binary | Role |
| --- | --- | --- |
| `crates/backend` | `iron-file-backend` | Singleton filesystem service and gRPC server (`crates/backend/src/main.rs`) |
| `crates/iced-frontend` | `iron-file-iced` | Default GUI, also runs as the file/folder picker |
| `crates/gtk-frontend` | `iron-file-gtk` | Alternative GTK4 GUI |
| `crates/common` | library `iron-file-common` | Protobuf types, gRPC client, socket/launcher logic, config/profile system |
| `crates/portal-filechooser` | `xdg-desktop-portal-iron-file` | D-Bus `FileChooser` and `OpenURI` portal backend, launches the Iced picker |

`proto/file_browser.proto` defines the `FileBrowser` gRPC service and is the
contract between the backend and every client; regenerate nothing by hand —
`crates/common/build.rs` compiles it via `tonic-prost-build` on every build.

## Commands

Enter the Nix dev shell first (`direnv allow` or `nix develop`) — it provides
`cargo`, `protobuf`, `pkg-config`, and the GTK/Wayland/Vulkan runtime libs, and
puts `scripts/` on `PATH`. Run `scripts/help` (or `help` once `scripts/` is on
PATH) to see this list from the shell itself.

```sh
cargo check --workspace          # type-check everything
cargo build --workspace          # build everything
cargo test --workspace           # run all tests
cargo test -p iron-file-common   # test one crate (config.rs has the bulk of unit tests)
cargo fmt / cargo fmt --check    # format / verify formatting
```

Run/dev scripts (each starts the backend automatically if not already running):

```sh
run-backend              # start only the backend
run [PATH]                # start the Iced frontend (kills any running backend first, then reconnects)
run-gtk [PATH]             # start the GTK4 frontend
pick-file / pick-files / pick-folder / pick-folders [PATH]   # picker-mode shortcuts
save-file NAME [PATH]      # single-folder picker that prints one destination path
save-files NAME... -- [PATH]
serve-docs                 # mdBook docs at docs/ (source of truth for user-facing docs)
```

`scripts/run` unconditionally `pkill -f iron-file-backend` before launching —
expect the backend to restart when using it during development.

## Architecture notes

**Backend is a singleton, enforced twice.** `socket_path()` in
`crates/common/src/lib.rs` resolves to `$IRON_FILE_SOCKET`, else
`$XDG_RUNTIME_DIR/iron-file-backend.sock`, else a temp-dir fallback. A second
backend refuses to bind while the socket is live; an flock-based lock file
(`backend_lock_path`) additionally prevents concurrent-startup races. Clients
that can't connect acquire a startup lock, spawn the backend (`dev` mode runs
`cargo run --manifest-path <workspace>/Cargo.toml -p iron-file-backend`;
`prod` mode execs `$IRON_FILE_BACKEND_BIN`), and poll the socket for up to 5s.

**Stale-client recovery.** Several `iron-file-common` RPC wrappers
(`copy_entries`, `rename_entry`) detect `tonic::Code::Unimplemented` — the
signature of a frontend whose proto has drifted from a backend left running
from before a rebuild — and transparently `restart_backend()` (SIGKILL by
`pkill -f iron-file-backend`, then reconnect) before retrying once. Keep this
pattern in mind when adding new RPCs: an old backend must fail closed with
`Unimplemented`, not silently misbehave.

**Everything crosses the wire as `FileEntry`/`BrowseResponse`.** The backend
does path resolution, symlink/metadata inspection, thumbnailing (images via
`image`, PDFs via `hayro`, STL via `vendor/stl-thumb`, video via `ffmpeg`),
archive compression/extraction (`zip`), and text-file previews (capped at
`MAX_PREVIEW_BYTES` = 1 MB) entirely server-side; frontends never touch the
filesystem directly except through the gRPC client in `iron-file-common`.

**Picker mode is the same Iced binary, not a separate tool.** `iron-file-iced
--mode picker [--file|--folder] [--single|--multiple] [--save-name NAME]`
stays attached to the invoking terminal, writes selected paths to stdout (one
per line), and exits 0 on confirm / no output on cancel. The portal backend
(`crates/portal-filechooser`) is the primary consumer: it's D-Bus-activated as
`org.freedesktop.impl.portal.desktop.iron-file`, translates portal
`FileChooser`/`OpenURI` requests into picker CLI flags, spawns the picker, and
maps stdout paths to `file://` URIs. See `docs/src/picker.md` and
`docs/src/xdg-desktop-portal.md` for the full flag/behavior reference before
changing picker semantics — those docs are user-facing and must stay in sync
with `crates/iced-frontend/src/main.rs` argument parsing.

**Configuration is a profile system, not a flat settings file.**
`crates/common/src/config.rs` (`ConfigStore`) loads TOML profiles from
`$XDG_CONFIG_HOME/iron-file/profiles/*.toml` plus system dirs under
`$XDG_CONFIG_DIRS`, each optionally inheriting from a `base_profile`. Missing
fields fall back through `config/default.toml`, embedded via `include_str!`
in `config.rs`. Read-only (system) profiles can't be edited in place: saving a
change to one creates a per-user "overlay" profile
(`<stem>-override-<hash>.toml`) with `base_profile` pointing at the original,
rather than mutating it. Follow this overlay pattern for any new
profile-editable setting.

**Vendored/patched Iced.** `vendor/iced-graphics`, `vendor/iced-wgpu`, and
`vendor/iced-widget` are patched forks selected via `[patch.crates-io]` in the
root `Cargo.toml`. `patches/*.patch` document *why* (backdrop blur support for
context menus, scroll-step/smooth-scroll APIs) — treat these as the crates to
edit directly when touching rendering/scrolling behavior; the `.patch` files
are for auditability/rebasing, not applied automatically.

**Config-driven UI surfaces.** Context-menu items (`ContextMenuItem`),
quick-toolbar items (`QuickToolbarItem`), sort orders, and keyboard shortcuts
are all enum-driven allowlists defined in `config.rs` with `ALL` /
`FILE_OPTIONS` / `FOLDER_OPTIONS` const arrays, serialized as kebab-case
strings. Adding a new menu/toolbar action means extending the enum, its
`Display` impl, its membership in the relevant const array, and the default
list in `config/default.toml`/`default_*` functions — all four must move
together.
