# iron-file

Basic Iced GUI example with message-based communication between the main GUI
thread and a background worker thread. Press the button to send an event to the
worker; it replies with a status update displayed in the window.

On Linux, the app uses X11 when `DISPLAY` is available. This avoids selecting
an unusable Wayland backend in environments that expose both display variables.

## NixOS development

The included `flake.nix` provides the X11, Wayland, Vulkan, and Rust runtime
dependencies required by Iced. With direnv and nix-direnv installed, run:

```sh
direnv allow
```

Alternatively, enter the shell explicitly with `nix develop`. Then run the
application with `cargo run`.
