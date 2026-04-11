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
        version = "2.0.7";

        assets = {
          x86_64-linux = {
            url = "https://github.com/mornepousse/KeSp_controller/releases/download/v${version}/KeSp_controller-linux-x86_64";
            hash = "sha256-wqHPPhVpK9g0RqV9pMnZ+Buy1khMS+a5jZp0x9UAbGU=";
          };
          aarch64-darwin = {
            url = "https://github.com/mornepousse/KeSp_controller/releases/download/v${version}/KeSp_controller-macos-arm64";
            hash = "sha256-C5u3COlm/JXVMmt7mDl76JfuDCidm1dGh8/lIh03UGc=";
          };
        };

        # Runtime dependencies for Slint UI
        runtimeLibs = with pkgs; [
          fontconfig
          libxkbcommon
          wayland
          udev
          stdenv.cc.cc.lib
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
            mkdir -p $out/bin $out/share/applications
            cp $src $out/bin/KeSp_controller
            chmod +x $out/bin/KeSp_controller

            cat > $out/share/applications/kesp-controller.desktop << EOF
            [Desktop Entry]
            Type=Application
            Name=KeSp Controller
            Comment=Cross-platform configurator for the KeSp split ergonomic keyboard
            Exec=KeSp_controller
            Icon=preferences-desktop-keyboard
            Categories=Utility;HardwareSettings;
            EOF
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
