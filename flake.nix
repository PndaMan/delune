{
  description = "delune: find music on Soulseek, check it, and file it into Navidrome";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAllSystems (pkgs: rec {
        delune = pkgs.callPackage ./nix/package.nix { };
        default = delune;
      });

      nixosModules = rec {
        delune = import ./nix/module.nix self;
        default = delune;
      };

      checks = forAllSystems (pkgs: {
        module = pkgs.testers.runNixOSTest (import ./nix/test.nix self);
      });

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages = with pkgs; [
            cargo
            rustc
            clippy
            rustfmt
            bun
            nodejs
          ];
        };
      });

      formatter = forAllSystems (pkgs: pkgs.nixfmt);
    };
}
