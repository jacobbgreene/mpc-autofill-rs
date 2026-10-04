{
  description = "MPC Autofill — a Rust/Tauri desktop tool for auto-filling card orders into MakePlayingCards";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { nixpkgs, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };

        rust = with pkgs; [ rustc cargo rustfmt clippy rust-analyzer ];

        # Native libraries Tauri's WebKitGTK webview links against.
        guiLibs = with pkgs; [
          glib
          gtk3
          cairo
          pango
          gdk-pixbuf
          atkmm
          harfbuzz
          librsvg
          libsoup_3
          webkitgtk_4_1
          openssl
          # GSettings schemas (e.g. org.gtk.Settings.FileChooser) GTK reads at runtime.
          gsettings-desktop-schemas
          # GIO TLS backend — without it the webview can't fetch https images.
          glib-networking
        ];
      in
      {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "mpc-autofill";
          version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).workspace.package.version;
          src = ./.;

          cargoLock.lockFile = ./Cargo.lock;
          buildFeatures = [ "tauri/custom-protocol" ];

          npmDeps = pkgs.fetchNpmDeps {
            src = ./frontend;
            hash = "sha256-OvX7I8mkPQNg3/SZtQHY5ixsVcJgJDyr48KsFGWS8Ro=";
          };
          npmRoot = "frontend";

          nativeBuildInputs = with pkgs; [
            nodejs_22
            npmHooks.npmConfigHook
            pkg-config
            wrapGAppsHook3
          ];
          buildInputs = guiLibs;

          preBuild = ''
            pushd "$npmRoot"
            npm run build
            popd
          '';

          cargoBuildFlags = [ "-p" "mpc-autofill" "--bin" "mpc-autofill" ];

          OPENSSL_NO_VENDOR = 1;
          doCheck = false;

          preFixup = ''
            gappsWrapperArgs+=(
              --set-default CHROME "${pkgs.chromium}/bin/chromium"
              --set WEBKIT_DISABLE_DMABUF_RENDERER "1"
              --prefix GIO_EXTRA_MODULES : "${pkgs.glib-networking}/lib/gio/modules"
              --prefix XDG_DATA_DIRS : "${pkgs.gtk3}/share/gsettings-schemas/${pkgs.gtk3.name}:${pkgs.gsettings-desktop-schemas}/share/gsettings-schemas/${pkgs.gsettings-desktop-schemas.name}"
            )
          '';
        };

        # Fast shell for the pure-logic crates — no WebKitGTK to fetch.
        devShells.core = pkgs.mkShell {
          packages = rust ++ [ pkgs.pkg-config pkgs.openssl pkgs.nodejs_22 ];
          OPENSSL_NO_VENDOR = "1";
        };

        # Full shell for building and running the Tauri app.
        devShells.default = pkgs.mkShell {
          packages = rust ++ (with pkgs; [ pkg-config nodejs_22 chromium cargo-tauri ]) ++ guiLibs;

          # chromiumoxide launches this browser for automation.
          CHROME = "${pkgs.chromium}/bin/chromium";
          OPENSSL_NO_VENDOR = "1";
          # WebKitGTK's DMABUF renderer misbehaves under many NixOS GPU setups.
          WEBKIT_DISABLE_DMABUF_RENDERER = "1";

          # Put GTK's compiled GSettings schemas on the data path; without this
          # GLib aborts with "schema 'org.gtk.Settings.FileChooser' is not installed".
          # GIO_EXTRA_MODULES points the webview at glib-networking's TLS backend so
          # it can load https images (Drive thumbnails).
          shellHook = ''
            export XDG_DATA_DIRS="${pkgs.gtk3}/share/gsettings-schemas/${pkgs.gtk3.name}:${pkgs.gsettings-desktop-schemas}/share/gsettings-schemas/${pkgs.gsettings-desktop-schemas.name}:$XDG_DATA_DIRS"
            export GIO_EXTRA_MODULES="${pkgs.glib-networking}/lib/gio/modules''${GIO_EXTRA_MODULES:+:$GIO_EXTRA_MODULES}"
          '';
        };
      });
}
