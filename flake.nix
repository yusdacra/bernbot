{
  inputs.nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
  inputs.nci.url = "github:yusdacra/nix-cargo-integration";
  inputs.nci.inputs.nixpkgs.follows = "nixpkgs";
  inputs.parts.url = "github:hercules-ci/flake-parts";
  inputs.parts.inputs.nixpkgs-lib.follows = "nixpkgs";

  outputs = inputs @ {
    parts,
    nci,
    ...
  }:
    parts.lib.mkFlake {inherit inputs;} {
      systems = ["x86_64-linux"];
      imports = [nci.flakeModule];
      perSystem = {
        pkgs,
        config,
        ...
      }: let
        crateName = "bernbot";
        crateOutputs = config.nci.outputs.${crateName};
      in {
        nci.projects.${crateName}.path = ./.;
        # nci.crates.${crateName} = {
        # };
        devShells.default = crateOutputs.devShell;
        packages.default = crateOutputs.packages.release;
      };
    };
}
