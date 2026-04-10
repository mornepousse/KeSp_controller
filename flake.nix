{
  description = "KeSp split keyboard configurator — Slint UI";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};

        nativeBuildInputs = with pkgs; [
          pkg-config
          cmake
        ];

        buildInputs = with pkgs; [
          fontconfig
          libxkbcommon
          wayland
          udev
        ] ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
          xorg.libX11
          xorg.libXcursor
          xorg.libXrandr
          xorg.libXi
        ];
      in
      {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "kesp-controller";
          version = "1.0.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;

          inherit nativeBuildInputs buildInputs;

          # Slint needs to find fontconfig at runtime
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath buildInputs;

          meta = with pkgs.lib; {
            description = "Cross-platform configurator for the KeSp split ergonomic keyboard";
            license = licenses.gpl3Only;
            mainProgram = "KeSp_controller";
          };
        };

        devShells.default = pkgs.mkShell {
          inherit nativeBuildInputs buildInputs;
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath buildInputs;
          packages = with pkgs; [ cargo rustc rust-analyzer clippy ];
        };
      });
}
