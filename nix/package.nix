{
  lib,
  stdenvNoCC,
  rustPlatform,
  bun,
  nodejs,
}:

let
  version = "0.1.0-dev";

  # Web dependencies, fetched once with bun and pinned by hash. When `web/bun.lock`
  # changes, update `outputHash` with the value Nix prints.
  webDeps = stdenvNoCC.mkDerivation {
    pname = "delune-web-deps";
    inherit version;
    src = lib.fileset.toSource {
      root = ../web;
      fileset = lib.fileset.unions [
        ../web/package.json
        ../web/bun.lock
      ];
    };
    nativeBuildInputs = [ bun ];
    dontConfigure = true;
    buildPhase = ''
      export HOME=$TMPDIR
      bun install --frozen-lockfile --no-progress --ignore-scripts
    '';
    installPhase = ''
      mkdir -p $out
      cp -r node_modules $out/
    '';
    dontFixup = true;
    outputHashMode = "recursive";
    outputHashAlgo = "sha256";
    outputHash = "sha256-ntG7FS8qe3Uq+5EW1eWN56lMqmSh7oTO53OTY08k7GI=";
  };

  web = stdenvNoCC.mkDerivation {
    pname = "delune-web";
    inherit version;
    src = lib.fileset.toSource {
      root = ../web;
      fileset = lib.fileset.difference ../web (lib.fileset.maybeMissing ../web/node_modules);
    };
    nativeBuildInputs = [
      bun
      nodejs
    ];
    buildPhase = ''
      export HOME=$TMPDIR
      cp -r ${webDeps}/node_modules node_modules
      chmod -R u+w node_modules
      patchShebangs node_modules
      bun run build
    '';
    installPhase = ''
      cp -r dist $out
    '';
  };
in
rustPlatform.buildRustPackage {
  pname = "delune";
  inherit version;

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../crates
    ];
  };
  cargoLock.lockFile = ../Cargo.lock;
  cargoBuildFlags = [
    "-p"
    "delune"
    "-p"
    "delune-tui"
  ];

  # The web UI is embedded into the binary at compile time.
  preBuild = ''
    mkdir -p web
    cp -r ${web} web/dist
  '';

  # Integration tests open sockets and talk to fake servers; CI runs them.
  doCheck = false;

  passthru = { inherit web webDeps; };

  meta = {
    description = "Find music on Soulseek, check it, and file it into Navidrome";
    homepage = "https://github.com/PndaMan/delune";
    license = lib.licenses.agpl3Plus;
    mainProgram = "delune";
    platforms = lib.platforms.linux;
  };
}
