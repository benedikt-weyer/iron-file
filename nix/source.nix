# Source filtered down to what cargo needs, so edits to docs, scripts, or
# Nix files do not invalidate the dependency or workspace builds.
{ lib, craneLib, self }:
let
  root = self;
  extraFiles = [
    "proto"   # compiled by crates/common/build.rs
    "vendor"  # patched crates, incl. shaders/fonts/assets
    "config"  # default.toml is include_str!'d
    "assets"
  ];
  keepExtra =
    path: _type:
    let
      rel = lib.removePrefix (toString root + "/") (toString path);
    in
    lib.any (d: rel == d || lib.hasPrefix (d + "/") rel) extraFiles;
in
lib.cleanSourceWith {
  src = root;
  filter = path: type: keepExtra path type || craneLib.filterCargoSources path type;
  name = "iron-file-source";
}
