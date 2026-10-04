{
  lib,
  self,
  craneLib,
  src,
  dummySrc,
  gtk4,
  makeWrapper,
  pkg-config,
  wrapGAppsHook4,
}:

let
  commonArgs = {
    pname = "iron-file-gtk";
    version = "0.1.0";
    inherit src;
    strictDeps = true;

    cargoExtraArgs = "--package iron-file-gtk --package iron-file-backend";

    nativeBuildInputs = [
      pkg-config
      wrapGAppsHook4
    ];

    buildInputs = [ gtk4 ];

    doCheck = false;
  };

  cargoArtifacts = craneLib.buildDepsOnly (commonArgs // { inherit dummySrc; });
in
craneLib.buildPackage (commonArgs // {
  inherit cargoArtifacts;

  nativeBuildInputs = commonArgs.nativeBuildInputs ++ [ makeWrapper ];

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

  meta = {
    description = "File browser built with GTK4";
    homepage = "https://github.com/benedikt-weyer/iron-file";
    license = lib.licenses.mit;
    mainProgram = "iron-file-gtk";
    platforms = lib.platforms.linux;
  };
})
