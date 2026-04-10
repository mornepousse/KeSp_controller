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
        version = "2.0.4";

        assets = {
          x86_64-linux = {
            url = "https://github.com/mornepousse/KeSp_controller/releases/download/v${version}/KeSp_controller-linux-x86_64";
            hash = "0h2l0xi4wm620zl559q9zvcbrarmfsf87gcj7qi8y4lzzw7sdjbc";
          };
          aarch64-darwin = {
            url = "https://github.com/mornepousse/KeSp_controller/releases/download/v${version}/KeSp_controller-macos-arm64";
            hash = "010l8y6jcbxsbarrv9ablx8gylavdx8iqrv9br7y5zpcqrwkjd55";
          };
        };

        # Runtime dependencies for Slint UI
        runtimeLibs = with pkgs; [
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
        packages.default = pkgs.stdenv.mkDerivation {
          pname = "kesp-controller";
          inherit version;

          src = pkgs.fetchurl assets.${system};

          nativeBuildInputs = with pkgs; [ autoPatchelfHook ];
          buildInputs = runtimeLibs;

          dontUnpack = true;

          installPhase = ''
            mkdir -p $out/bin
            cp $src $out/bin/KeSp_controller
            chmod +x $out/bin/KeSp_controller
          '';

          meta = with pkgs.lib; {
            description = "Cross-platform configurator for the KeSp split ergonomic keyboard";
            license = licenses.gpl3Only;
            mainProgram = "KeSp_controller";
            platforms = builtins.attrNames assets;
          };
        };

        # Dev shell for building from source
        devShells.default = pkgs.mkShell {
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtimeLibs;
          packages = with pkgs; [
            cargo rustc rust-analyzer clippy
            pkg-config cmake
          ] ++ runtimeLibs;
        };
      });
}
