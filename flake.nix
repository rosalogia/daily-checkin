{
  description = "Daily Check-in Discord Bot";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
        
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };

        # Extract package info from Cargo.toml
        manifest = (pkgs.lib.importTOML ./Cargo.toml).package;

        # Build inputs needed for the bot (removed sqlite)
        buildInputs = with pkgs; [
          pkg-config
          openssl
          libressl
        ];
      in
      {
        packages.default = pkgs.rustPlatform.buildRustPackage {
            pname = manifest.name;
            version = manifest.version;

            src = pkgs.lib.cleanSource ./.;
            cargoLock.lockFile = ./Cargo.lock;

            nativeBuildInputs = buildInputs;
            buildInputs = buildInputs;

            env = {
              PKG_CONFIG_PATH = "${pkgs.openssl.dev}/lib/pkgconfig";
            };

            # Skip tests during build as they may require network access
            doCheck = false;

            meta = with pkgs.lib; {
              description = manifest.description or "Discord bot for daily check-ins and streak tracking";
              license = licenses.mit;
              maintainers = [ ];
            };
        };

        apps.default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/daily-checkin-bot";
        };

        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            rustToolchain
            cargo-watch
            cargo-edit
          ] ++ buildInputs;

          env = {
            RUST_BACKTRACE = "1";
            PKG_CONFIG_PATH = "${pkgs.openssl.dev}/lib/pkgconfig";
          };

          shellHook = ''
            echo "Daily Check-in Discord Bot Development Environment"
            echo "Rust version: $(rustc --version)"
            echo "Cargo version: $(cargo --version)"
            echo ""
            echo "Available commands:"
            echo "  cargo run              - Run the bot (requires .env file)"
            echo "  cargo watch -x run     - Auto-rebuild and run on changes"
            echo "  nix build              - Build release binary"
            echo "  nix run                - Run the built binary"
          '';
        };
      }
    )
    {
      nixosModules.daily-checkin = import ./nix/daily-checkin-module.nix;
    };
}
