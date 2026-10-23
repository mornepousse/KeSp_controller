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
        version = "2.2.0";

        # Binaries come from the GitHub release assets — public, no token.
        # URL format: https://github.com/<owner>/<repo>/releases/download/v<version>/<file>
        #
        # Le hash adresse le CONTENU : il doit etre releve sur l'asset REEL de
        # la version, apres publication, et change dans le MEME commit que
        # `version`. Au bump v2.1.0 la version avait ete montee sans le hash,
        # et nix a servi le binaire de v2.0.9 pendant cinq mois sans erreur.
        # Voir la section Release du CLAUDE.md.
        ghRelease = "https://github.com/mornepousse/KeSp_controller/releases/download";

        assets = {
          x86_64-linux = {
            url = "${ghRelease}/v${version}/KeSp_controller-linux-x86_64";
            hash = "sha256-GtafNfIEj81rL+LXDdC3PalatJy3t9GgXbaGkIBxAwM=";
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
        ] ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [
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
