{
  description = "Receipt Dropbox: Rust/WebAssembly core with Nickel plans";

  inputs = {
    nixpkgs.url = "https://flakehub.com/f/NixOS/nixpkgs/0.1"; # unstable Nixpkgs
    fenix = {
      url = "https://flakehub.com/f/nix-community/fenix/0.1";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { self, ... }@inputs:

    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      forEachSupportedSystem =
        f:
        inputs.nixpkgs.lib.genAttrs supportedSystems (
          system:
          f {
            inherit system;
            pkgs = import inputs.nixpkgs {
              inherit system;
              overlays = [ inputs.self.overlays.default ];
            };
          }
        );
    in
    {
      overlays.default = final: prev: {
        rustToolchain =
          let
            fenix = inputs.fenix.packages.${prev.stdenv.hostPlatform.system};
          in
          fenix.combine (
            with fenix.stable;
            [
              clippy
              rustc
              cargo
              rustfmt
              rust-src
              fenix.targets.wasm32-unknown-unknown.stable.rust-std
              fenix.targets.wasm32-wasip2.stable.rust-std
            ]
          );
      };

      devShells = forEachSupportedSystem (
        { pkgs, system }:
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              rustToolchain
              rust-analyzer
              cargo-deny
              cargo-watch
              # WebAssembly: run and inspect components, bind for the browser.
              wasmtime
              wasm-tools
              wasm-bindgen-cli
              # Nickel CLI and language server for writing plans and contracts; the core embeds
              # its own interpreter through nickel-embed.
              nickel
              nls
              # Renders viewable statements (piece 10); the core will embed it as a crate.
              typst
              self.formatter.${system}
            ];

            env.RUST_SRC_PATH = "${pkgs.rustToolchain}/lib/rustlib/src/rust/library";
          };
        }
      );

      formatter = forEachSupportedSystem ({ pkgs, ... }: pkgs.nixfmt);

      # `nix flake check`: the dev shell builds, and the Nix is formatted. Cargo's checks run
      # inside the shell (`cargo xtask ci`), where they can reach crates.io and git.
      checks = forEachSupportedSystem (
        { pkgs, system }:
        {
          devShell = self.devShells.${system}.default;
          formatting = pkgs.runCommand "nixfmt-check" { } ''
            ${pkgs.nixfmt}/bin/nixfmt --check ${./flake.nix}
            touch $out
          '';
        }
      );
    };
}
