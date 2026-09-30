{
  description = "Reignite: opt-in recovery for interrupted coding agents";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  outputs = { self, nixpkgs }: let
    forLinux = nixpkgs.lib.genAttrs [ "aarch64-linux" "x86_64-linux" ];
    pkgsFor = system: import nixpkgs { inherit system; };
  in {
    packages = forLinux (system: let pkgs = pkgsFor system; in {
      default = pkgs.rustPlatform.buildRustPackage {
        pname = "reignite";
        version = "0.1.0";
        # Documentation and adapter changes do not invalidate the Rust package.
        src = pkgs.lib.fileset.toSource {
          root = ./.;
          fileset = pkgs.lib.fileset.unions [
            ./Cargo.toml ./Cargo.lock ./src ./tests/cli.rs
          ];
        };
        cargoLock.lockFile = ./Cargo.lock;
      };
    });
    checks = forLinux (system: {
      package = self.packages.${system}.default;
    });
    apps = forLinux (system: {
      default = {
        type = "app";
        program = "${self.packages.${system}.default}/bin/reignite";
        meta.description = "Inspect and authorize opt-in coding-agent recovery";
      };
    });
    devShells = forLinux (system: let pkgs = pkgsFor system; in {
      default = pkgs.mkShell {
        packages = with pkgs; [ cargo rustc rustfmt clippy nodejs typescript python3 tmux jq git shellcheck actionlint ];
      };
    });
  };
}
