{
  inputs = {
    naersk = {
      url = "github:yusdacra/naersk/feat/git-submodule";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    nixCargoIntegration = {
      url = "github:yusdacra/nix-cargo-integration";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.naersk.follows = "naersk";
    };
  };

  outputs = inputs:
    let
      lib = inputs.nixpkgs.lib;
      mkPlatform = platform:
        let
          outputs = inputs.nixCargoIntegration.lib.makeOutputs {
            root = ./.;
            overrides = {
              build = common: prevb: {
                allRefs = true;
                submodules = true;
                nativeBuildInputs = prevb.nativeBuildInputs ++ [ common.pkgs.rustfmt ];
                name = "${prevb.name}-${platform}";
                cargoBuildOptions = def: ((prevb.cargoBuildOptions or (_: [ ])) def) ++ [ "--features" platform ];
              };
            };
          };
        in
        lib.mapAttrs
          (name: attrs:
            if lib.any (x: x == name) [ "apps" "checks" "packages" ]
            then lib.mapAttrs (_: lib.mapAttrs' (name: lib.nameValuePair "${name}-${platform}")) attrs
            else attrs
          )
          outputs;
      platforms = map mkPlatform [ "discord" "harmony" ];
      outputs = lib.foldAttrs lib.recursiveUpdate { } platforms;
    in
    outputs // {
      defaultPackage = lib.mapAttrs (_: pkgs: pkgs.bernbot-discord) outputs.packages;
      defaultApp = lib.mapAttrs (_: apps: apps.bernbot-discord) outputs.apps;
    };
}
