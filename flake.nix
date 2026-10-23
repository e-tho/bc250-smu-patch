{
  description = "Patch the AMD BC-250 SMU firmware";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" ];
      forAllSystems =
        f:
        nixpkgs.lib.genAttrs systems (
          system:
          f {
            inherit system;
            pkgs = import nixpkgs { inherit system; };
          }
        );
    in
    {
      packages = forAllSystems (
        { pkgs, ... }: {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "bc250-smu-patch";
            version = "0.1.0";
            src = ./.;

            cargoLock.lockFile = ./Cargo.lock;

            meta = {
              description = "Patch the AMD BC-250 SMU firmware";
              license = pkgs.lib.licenses.mit;
              platforms = pkgs.lib.platforms.linux;
              mainProgram = "bc250-smu-patch";
            };
          };
        }
      );

      devShells = forAllSystems (
        { pkgs, ... }: {
          default = pkgs.mkShell {
            packages = with pkgs; [
              cargo
              rustc
              rustfmt
              clippy
              rust-analyzer
              rustPlatform.rustLibSrc
            ];
          };
        }
      );
    };
}
