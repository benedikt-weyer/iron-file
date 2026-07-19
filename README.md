# iron-file

Basic GUI application with message-based communication between the main GUI
thread and a shared background worker. Press the button to send an event to the
worker; it replies with a status update displayed in the window. Iced is the
default frontend, with GTK4 available as an optional build target.

On Linux, the app uses X11 when `DISPLAY` is available. This avoids selecting
an unusable Wayland backend in environments that expose both display variables.

## NixOS development

The included `flake.nix` provides the X11, Wayland, Vulkan, GTK4, and Rust
runtime dependencies. With direnv and nix-direnv installed, run:

```sh
direnv allow
```

Alternatively, enter the shell explicitly with `nix develop`. Then run the
application with `cargo run` or the `run` wrapper.

## GTK4 frontend

Build and run the non-default GTK4 target with:

```sh
cargo run --bin iron-file-gtk --features gtk4
```

Both frontends use the same channel-based backend in `src/lib.rs`.
Inside the direnv shell, `run-gtk` provides the same GTK4 command.
