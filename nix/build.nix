{ release ? false
, doCheck ? false
, doDoc ? false
, common
,
}:
with common;
let
  meta = with pkgs.lib; {
    description = "bernbot is a Rust project.";


    license = licenses.gpl3;
  };



  package = with pkgs; naersk.buildPackage {
    root = ../.;
    nativeBuildInputs = crateDeps.nativeBuildInputs;
    buildInputs = crateDeps.buildInputs;
    # WORKAROUND doctests fail to compile (they compile with nightly cargo but then rustdoc fails)
    cargoTestOptions = def: def ++ [ "--tests" "--bins" "--examples" ];
    override = (prev: (lib.listToAttrs (map (e: { "${e.name}" = e.value; }) env)));
    overrideMain = (prev: {
      inherit meta;

    });

    inherit release doCheck doDoc;
  };
in
package
