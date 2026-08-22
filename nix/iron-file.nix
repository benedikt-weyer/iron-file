{
  lib,
  self,
  libGL,
  libX11,
  libXcursor,
  libXi,
  libXrandr,
  libXrender,
  libxkbcommon,
  copyDesktopItems,
  cmake,
  ffmpeg,
  fontconfig,
  glib,
  makeWrapper,
  makeDesktopItem,
  pkg-config,
  rustPlatform,
  vulkan-loader,
  wayland,
}:

rustPlatform.buildRustPackage {
  pname = "iron-file";
  version = "0.1.0";

  src = self;

  cargoLock.lockFile = "${self}/Cargo.lock";

  env.CMAKE_POLICY_VERSION_MINIMUM = "3.5";

  cargoBuildFlags = [
    "--package"
    "iron-file-iced"
    "--package"
    "iron-file-backend"
    "--package"
    "xdg-desktop-portal-iron-file"
  ];

  nativeBuildInputs = [
    cmake
    copyDesktopItems
    makeWrapper
    pkg-config
  ];

  buildInputs = [
    fontconfig
    glib
  ];

  desktopItems = [
    (makeDesktopItem {
      name = "iron-file";
      desktopName = "Iron File";
      comment = "File browser";
      exec = "iron-file-iced";
      icon = "iron-file";
      categories = [
        "System"
        "FileManager"
      ];
      mimeTypes = [ "inode/directory" ];
    })
  ];

  postInstall = ''
    ln -s iron-file-iced "$out/bin/iron-file"
    install -Dm755 "$out/bin/iron-file-backend" \
      "$out/libexec/iron-file/iron-file-backend"
    install -Dm644 "${self}/assets/iron-file.svg" \
      "$out/share/icons/hicolor/scalable/apps/iron-file.svg"
    rm "$out/bin/iron-file-backend"
    install -Dm644 -T /dev/stdin \
      "$out/share/xdg-desktop-portal/portals/iron-file.portal" <<'EOF'
[portal]
DBusName=org.freedesktop.impl.portal.desktop.iron-file
Interfaces=org.freedesktop.impl.portal.FileChooser;org.freedesktop.impl.portal.OpenURI
UseIn=GNOME
EOF
    install -Dm644 -T /dev/stdin \
      "$out/share/dbus-1/services/org.freedesktop.impl.portal.desktop.iron-file.service" <<EOF
[D-BUS Service]
Name=org.freedesktop.impl.portal.desktop.iron-file
Exec=$out/bin/xdg-desktop-portal-iron-file
EOF
    install -Dm644 -T /dev/stdin \
      "$out/share/dbus-1/services/org.freedesktop.FileManager1.service" <<EOF
[D-BUS Service]
Name=org.freedesktop.FileManager1
Exec=$out/bin/xdg-desktop-portal-iron-file
EOF
  '';

  preFixup = ''
    wrapProgram "$out/bin/iron-file-iced" \
      --set IRON_FILE_BACKEND_MODE prod \
      --set IRON_FILE_BACKEND_BIN "$out/libexec/iron-file/iron-file-backend" \
      --set IRON_FILE_FFMPEG "${ffmpeg}/bin/ffmpeg" \
      --prefix LD_LIBRARY_PATH : "${
        lib.makeLibraryPath [
          libGL
          libX11
          libXcursor
          libXi
          libXrandr
          libXrender
          libxkbcommon
          vulkan-loader
          wayland
        ]
      }"
    wrapProgram "$out/bin/xdg-desktop-portal-iron-file" \
      --set IRON_FILE_BIN "$out/bin/iron-file-iced"
  '';

  doCheck = false;

  meta = {
    description = "File browser built with Iced";
    homepage = "https://github.com/benedikt-weyer/iron-file";
    license = lib.licenses.mit;
    mainProgram = "iron-file";
    platforms = lib.platforms.linux;
  };
}
