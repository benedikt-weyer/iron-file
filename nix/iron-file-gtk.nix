{
  lib,
  self,
  gtk4,
  makeWrapper,
  pkg-config,
  rustPlatform,
  wrapGAppsHook4,
}:

rustPlatform.buildRustPackage {
  pname = "iron-file-gtk";
  version = "0.1.0";

  src = self;

  cargoLock.lockFile = "${self}/Cargo.lock";

  cargoBuildFlags = [
    "--package"
    "iron-file-gtk"
    "--package"
    "iron-file-backend"
  ];

  nativeBuildInputs = [
    makeWrapper
    pkg-config
    wrapGAppsHook4
  ];

  buildInputs = [ gtk4 ];

  postInstall = ''
    install -Dm755 "$out/bin/iron-file-backend" \
      "$out/libexec/iron-file/iron-file-backend"
    rm "$out/bin/iron-file-backend"
  '';

  preFixup = ''
    wrapProgram "$out/bin/iron-file-gtk" \
      --set IRON_FILE_BACKEND_MODE prod \
      --set IRON_FILE_BACKEND_BIN "$out/libexec/iron-file/iron-file-backend"
  '';

  doCheck = false;

  meta = {
    description = "File browser built with GTK4";
    homepage = "https://github.com/benedikt-weyer/iron-file";
    license = lib.licenses.mit;
    mainProgram = "iron-file-gtk";
    platforms = lib.platforms.linux;
  };
}
