{ common }:
with common; with pkgs;
devshell.mkShell {
  packages = [ rustc ] ++ crateDeps.nativeBuildInputs ++ crateDeps.buildInputs;
  commands =
    let
      pkgCmd = pkg: { package = pkg; };
    in
    [
      (pkgCmd git)
      (pkgCmd nixpkgs-fmt)

    ];
  env = with lib; [

    (nameValuePair "LD_LIBRARY_PATH" "$LD_LIBRARY_PATH:${lib.makeLibraryPath neededLibs}")
  ] ++ env;
}
