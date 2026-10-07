{
  description = "Daily Check-in Discord Bot";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils }:
    (flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        # Shared with CI via rust-toolchain.toml
        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            rustToolchain
            cargo-watch
            cargo-edit
            cargo-lambda
            zig
            awscli2
            aws-sam-cli
          ];

          env = {
            RUST_BACKTRACE = "1";
          };

          shellHook = ''
            echo "Daily Check-in Discord Bot Development Environment"
            echo "Rust version: $(rustc --version)"
            echo "Cargo version: $(cargo --version)"
            echo ""
            echo "Available commands:"
            echo "  cargo test                             - Run tests"
            echo "  sam build && sam deploy                - Build and deploy to AWS"
            echo "  cargo run --bin register-commands      - Register slash commands (requires .env)"
            echo "  cargo run --bin import-data -- <file>  - Import a bot_data.json into DynamoDB"
          '';
        };
      }
    ));
}
