# XDG Desktop Portal

`xdg-desktop-portal-iron-file` implements
`org.freedesktop.impl.portal.FileChooser.OpenFile`. It translates the portal
options `directory` and `multiple` into Iron File picker flags and returns
selected locations as normalized `file://` URIs.

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
  };
};
```

The explicit FileChooser entry routes selection requests to Iron File. The
default list keeps GNOME and GTK as providers for portal interfaces that Iron
File does not implement.

After rebuilding NixOS, restart the user portal services or log in again:

```sh
systemctl --user restart xdg-desktop-portal.service
```

When updating the custom package source pin, refresh both the source hash and
`cargoHash` in `custom-packages/iron-file/package.nix`.
