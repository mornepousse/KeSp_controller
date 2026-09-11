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
        version = "2.1.0";

        # Binaries are hosted on GitLab generic package registry.
        # URL format: https://gitlab.com/api/v4/projects/<project>/packages/generic/kesp-controller/<version>/<file>
        gitlabPkg = "https://gitlab.com/api/v4/projects/harrael%2FKeSp_controller/packages/generic/kesp-controller";

        assets = {
          x86_64-linux = {
            url = "${gitlabPkg}/v${version}/KeSp_controller-linux-x86_64";
            hash = "sha256-Ga9BCwY04glRkoKNYGBOIgOuNfFRQcxzXxBoWfg074M=";
          };
        };

        # Runtime dependencies for Slint UI
        runtimeLibs = with pkgs; [
          fontconfig
          libxkbcommon
          wayland
          udev
          stdenv.cc.cc.lib
          # Couche de dispatch GL/EGL (libGL.so.1, libEGL.so.1, libGLdispatch.so.0).
          # Elle N'EST PAS dans /run/opengl-driver/lib : ce répertoire ne contient
          # que les implémentations fournisseur (libEGL_mesa.so.0,
          # libEGL_nvidia.so.0, libGLX_nvidia.so.0). Vérifié sur NixOS 26.05.
          # Sans ce paquet, glutin/winit ne trouve aucun point d'entrée GL.
          libGL
        ] ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
          libX11
          libXcursor
          libXrandr
          libXi
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
          # /run/opengl-driver/lib EN PREMIER : c'est là que NixOS expose les
          # pilotes graphiques du système (mesa, nvidia). runtimeLibs fournit le
          # dispatch, ce chemin fournit l'implémentation — il faut les DEUX.
          LD_LIBRARY_PATH = "/run/opengl-driver/lib:" + pkgs.lib.makeLibraryPath runtimeLibs;
          packages = with pkgs; [
            cargo rustc rust-analyzer clippy
            pkg-config cmake
          ] ++ runtimeLibs;
        };
      });
}
