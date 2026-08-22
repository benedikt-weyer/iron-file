# XDG Desktop Portal

`xdg-desktop-portal-iron-file` implements
`org.freedesktop.impl.portal.FileChooser.OpenFile`, `SaveFile`, `SaveFiles`,
`org.freedesktop.impl.portal.OpenURI.OpenFile`, and `OpenDirectory`.
FileChooser `OpenFile` translates the portal options `directory` and
`multiple` into Iron File picker flags and returns selected locations as
normalized `file://` URIs.

For save requests, Iron File presents a single-folder picker. `SaveFile` joins
that folder with the portal's `current_name`; the name is shown in an editable
input bar and can be reset to the portal-provided original. `SaveFiles` joins
the selected folder with every name in `files`, retaining their order. Names
must be simple file names, so the backend does not accept path traversal or
absolute paths from a portal caller.

OpenURI `OpenFile` opens the descriptor's resolved path with its associated
desktop application. `OpenDirectory` opens Iron File at the target directory,
or at a file's parent directory.

`xdg-desktop-portal-iron-file` also owns the well-known bus name
`org.freedesktop.FileManager1` (best-effort: if another file manager already
owns it, the request is skipped and the portal backend still starts) and
implements its `ShowFolders`, `ShowItems`, and `ShowItemProperties` methods.
All three take a list of `file://` URIs and open Iron File at the
corresponding directories, de-duplicated; `ShowItems` and
`ShowItemProperties` resolve each URI to its containing folder first. Iron
File has no item-properties dialog, so `ShowItemProperties` is handled
identically to `ShowItems` as a best-effort fallback rather than erroring.

## NixOS Package Contents

The custom Iron File Nix package installs the portal executable, its
`iron-file.portal` metadata file, and the D-Bus activation service. The service
is wrapped with `IRON_FILE_BIN` pointing to the packaged Iced frontend.

## NixOS Configuration

```nix
xdg.portal = {
  enable = true;
  extraPortals = [ pkgs-custom.iron-file ];
  config.common = {
    default = [ "gnome" "gtk" ];
    "org.freedesktop.impl.portal.FileChooser" = [ "iron-file" ];
    "org.freedesktop.impl.portal.OpenURI" = [ "iron-file" ];
  };
};
```

The explicit entries route file selection and local file-opening requests to
Iron File. The default list keeps GNOME and GTK as providers for portal
interfaces that Iron File does not implement.

After rebuilding NixOS, restart the user portal services or log in again:

```sh
systemctl --user restart xdg-desktop-portal.service
```

When updating the custom package source pin, refresh both the source hash and
`cargoHash` in `custom-packages/iron-file/package.nix`.
