{
  description = "Development environment for Iron File GUI applications";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  inputs.crane.url = "github:ipetkov/crane";

  outputs = { self, nixpkgs, crane }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in {
      devShells = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
          runtimeLibraries = with pkgs; [
            gtk4
            libGL
            libX11
            libXcursor
            libXi
            libXrandr
            libXrender
            libxkbcommon
            vulkan-loader
            wayland
          ];
        in {
          default = pkgs.mkShell {
            packages = with pkgs; [
              cargo
              pkg-config
              protobuf
              rustc
            ];

            buildInputs = runtimeLibraries;
            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtimeLibraries;

            # FHS-style wrappers (steam-run, some editor/agent sandboxes) export
            # NIX_CFLAGS_LINK=-L/usr/lib, which makes the linker pick the host's
            # glibc over this shell's. Binaries then require newer libm symbols
            # than their RUNPATH glibc provides and fail to start.
            shellHook = ''
              unset NIX_CFLAGS_LINK
            '';
          };
        });

      packages = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
          craneLib = crane.mkLib pkgs;
          src = import ./nix/source.nix { inherit (pkgs) lib; inherit craneLib self; };
          # buildDepsOnly stubs every workspace .rs file, including the patched
          # crates under vendor/. Registry crates such as iced_tiny_skia compile
          # against those, so keep vendor/ real in the dependency-only build.
          dummySrc = craneLib.mkDummySrc {
            inherit src;
            extraDummyScript = ''
              rm -rf $out/vendor
              cp -r ${src}/vendor $out/vendor
              chmod -R u+w $out/vendor
            '';
          };
        in {
          iron-file = pkgs.callPackage ./nix/iron-file.nix { inherit self craneLib src dummySrc; };
          iron-file-gtk = pkgs.callPackage ./nix/iron-file-gtk.nix { inherit self craneLib src dummySrc; };
          default = self.packages.${system}.iron-file;
        });
    };
}
