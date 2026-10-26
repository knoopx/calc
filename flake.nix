{
  description = "A plain-text notes sheet that computes: numbers, units, currencies and dates evaluated on every line as you type";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];

      forAllSystems =
        f:
        nixpkgs.lib.genAttrs systems (
          system:
          f (
            import nixpkgs {
              inherit system;
              config.allowUnfree = true;
            }
          )
        );

      gpuDriver =
        pkgs:
        pkgs.linuxPackages.nvidia_x11_latest.override {
          acceptLicense = true;
        };

      runtimeLibs =
        pkgs:
          with pkgs; [
            wayland
            libxkbcommon
            libglvnd
            vulkan-loader
            dbus
            (gpuDriver pkgs)
          ];

      envFor = pkgs: {
        LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (runtimeLibs pkgs);
        __EGL_VENDOR_LIBRARY_FILENAMES = "${gpuDriver pkgs}/share/glvnd/egl_vendor.d/10_nvidia.json";
        VK_ICD_FILENAMES = "${gpuDriver pkgs}/share/vulkan/icd.d/nvidia_icd.json";
      };

      desktopItem =
        pkgs:
        pkgs.makeDesktopItem {
          name = "calc";
          desktopName = "Calc";
          genericName = "Computing notes";
          comment = "A plain-text notes sheet that computes: numbers, units, currencies and dates evaluated on every line as you type";
          icon = "calc";
          exec = "calc";
          categories = [
            "Utility"
            "TextEditor"
          ];
          keywords = [
            "notes"
            "calculator"
            "maths"
            "units"
            "currency"
          ];
          startupNotify = true;
          startupWMClass = "calc";
        };

      package =
        pkgs:
        pkgs.rustPlatform.buildRustPackage (finalAttrs: {
          pname = "calc";
          version = (pkgs.lib.importTOML ./Cargo.toml).package.version;
          src = self;

          cargoLock.lockFile = ./Cargo.lock;

          nativeBuildInputs = with pkgs; [
            makeWrapper
            pkg-config
            copyDesktopItems
          ];
          buildInputs = runtimeLibs pkgs ++ [ pkgs.fontconfig ];

          desktopItems = [ (desktopItem pkgs) ];

          postInstall = ''
            install -Dm644 assets/icons/calc-icon.svg $out/share/icons/hicolor/scalable/apps/calc.svg
            install -Dm644 assets/icons/calc-icon-512.png $out/share/icons/hicolor/512x512/apps/calc.png
            install -Dm644 assets/icons/calc-icon-128.png $out/share/icons/hicolor/128x128/apps/calc.png
            install -Dm644 assets/icons/calc-icon-48.png $out/share/icons/hicolor/48x48/apps/calc.png
            install -Dm644 assets/icons/calc-icon-24.png $out/share/icons/hicolor/24x24/apps/calc.png
          '';

          postFixup = ''
            wrapProgram $out/bin/calc \
              --suffix LD_LIBRARY_PATH : "${pkgs.lib.makeLibraryPath (runtimeLibs pkgs)}" \
              --set __EGL_VENDOR_LIBRARY_FILENAMES "${gpuDriver pkgs}/share/glvnd/egl_vendor.d/10_nvidia.json" \
              --set VK_ICD_FILENAMES "${gpuDriver pkgs}/share/vulkan/icd.d/nvidia_icd.json" \
              --prefix PATH : "${pkgs.lib.makeBinPath [ pkgs.fontconfig ]}"
          '';

          meta = {
            description = "A plain-text notes sheet that computes: numbers, units, currencies and dates evaluated on every line as you type";
            license = pkgs.lib.licenses.mit;
            mainProgram = "calc";
            platforms = pkgs.lib.platforms.linux;
          };
        });
    in
    {
      packages = forAllSystems (pkgs: {
        calc = package pkgs;
        default = package pkgs;
      });

      overlays.default = final: prev: {
        calc = package final;
      };

      nixosModules.default =
        { pkgs, ... }:
        {
          environment.systemPackages = [ (package pkgs) ];
        };

      apps = forAllSystems (pkgs: {
        default = {
          type = "app";
          program = "${package pkgs}/bin/calc";
        };
      });

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShellNoCC (
          envFor pkgs
          // {
            name = "calc";
            packages = with pkgs; [
              fontconfig
              pkg-config
              wayland-scanner
            ];
          }
        );

        rust = pkgs.mkShell (
          envFor pkgs
          // {
            name = "calc-rust";
            packages = with pkgs; [
              cargo
              clippy
              rustc
              rustfmt
              pkg-config
              wayland-scanner
              fontconfig
            ];
            CARGO_TARGET_DIR = "target/nix";
          }
        );
      });

      formatter = forAllSystems (pkgs: pkgs.nixfmt-rfc-style);
    };
}
