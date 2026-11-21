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
        let
          # The OCR models rdx-ocr's tests read (piece 11, docs/adr/ADR-P11-ocr.md). Pinned by
          # hash; the Hugging Face mirror is pinned to a commit because the author's own bucket
          # refuses some networks. The product never bundles these: hosts put them in the blob
          # store, where `ocr.read` finds them by the same digests.
          ocrModel =
            name: hash:
            pkgs.fetchurl {
              urls = [
                "https://ocrs-models.s3-accelerate.amazonaws.com/${name}"
                "https://huggingface.co/Pieces/ocrs-mirror/resolve/fad3720109ad35b579f6bc6501290d1a2388ad11/${name}"
              ];
              inherit hash;
            };
          ocrModels = pkgs.linkFarm "ocrs-models" [
            {
              name = "text-detection.rten";
              path = ocrModel "text-detection.rten" "sha256-8Vz7Vr0CxL9HiiA0OYZQSh8B4WZcKzoK1mNA8FSxtco=";
            }
            {
              name = "text-recognition.rten";
              path = ocrModel "text-recognition.rten" "sha256-5ISGbUzOQDF1vY0AsSj+sIq0LiCN4w5CzZiJ2PFzWm4=";
            }
          ];
        in
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

            env = {
              RUST_SRC_PATH = "${pkgs.rustToolchain}/lib/rustlib/src/rust/library";
              RDX_OCR_MODELS = "${ocrModels}";
            };
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
