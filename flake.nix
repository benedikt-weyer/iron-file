{
  description = "Development environment for Iron File GUI applications";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
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
          };
        });

      packages = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
        in {
          iron-file = pkgs.callPackage ./nix/iron-file.nix { inherit self; };
          iron-file-gtk = pkgs.callPackage ./nix/iron-file-gtk.nix { inherit self; };
          default = self.packages.${system}.iron-file;
        });
    };
}
