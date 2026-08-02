# Architecture

| Component | Cargo package | Responsibility |
| --- | --- | --- |
| Iced frontend | `iron-file-iced` | Default graphical browser and picker UI |
| GTK frontend | `iron-file-gtk` | Alternative GTK4 browser UI |
| Backend | `iron-file-backend` | Singleton filesystem operations and gRPC service |
| Shared library | `iron-file-common` | Protobuf API, backend launcher, configuration, and client helpers |
| Portal backend | `xdg-desktop-portal-iron-file` | D-Bus FileChooser and OpenURI implementation for XDG Desktop Portal |

The frontend starts the backend when needed. The backend listens on
`$XDG_RUNTIME_DIR/iron-file-backend.sock`; when that variable is unavailable it
uses a temporary-directory socket.

The portal backend is D-Bus activated by
`org.freedesktop.impl.portal.desktop.iron-file`. For `OpenFile` requests it
starts `iron-file-iced --mode picker`, receives selected paths on standard
output, and returns them to XDG Desktop Portal as `file://` URIs.
