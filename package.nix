{ lib
, rustPlatform
}:

rustPlatform.buildRustPackage {
  pname = "pyroclear";
  version = (lib.importTOML ./Cargo.toml).package.version;

  src = ./.;

  cargoLock.lockFile = ./Cargo.lock;

  meta = {
    description = "Pyroclear";
    license = lib.licenses.mit;
    platforms = lib.platforms.linux;
  };
}
