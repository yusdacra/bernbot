{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    nixCargoIntegration = {
      url = "github:yusdacra/nix-cargo-integration";
      inputs.nixpkgs.follows = "nixpkgs";
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
                cargoBuildOptions = def: (prevb.cargoBuildOptions def) ++ [ "--no-default-features" "--features" platform ];
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
      defaultPackage.x86_64-linux = outputs.packages.x86_64-linux.bernbot-discord;
      defaultApp.x86_64-linux = outputs.apps.x86_64-linux.bernbot-discord;
    };
}
