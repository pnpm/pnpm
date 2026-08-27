{
  description = "Fast, disk space efficient package manager";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    # Pin x86_64-darwin to a stable darwin branch. nixpkgs-unstable
    # (26.11) dropped x86_64-darwin support; the -darwin branch
    # receives security updates without the breaking churn.
    nixpkgs-darwin-legacy.url = "github:NixOS/nixpkgs/nixpkgs-26.05-darwin";
    # crane provides Cargo workspace build support (dependency
    # vendoring from Cargo.lock, incremental compilation caching).
    crane.url = "github:ipetkov/crane";
    # rust-overlay provides Rust toolchains matching rust-toolchain.toml,
    # which nixpkgs's packaged Rust may lag behind.
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, nixpkgs-darwin-legacy, crane, rust-overlay }: let
    version = "12.0.0";

    assets = {
      "x86_64-linux" = {
        file = "pnpm-linux-x64.tar.gz";
        sha256 = "d93497ba07d5dfc7d527b30905fdd24f55c87618ea23ea3af287cdff061510e0";
      };
      "aarch64-linux" = {
        file = "pnpm-linux-arm64.tar.gz";
        sha256 = "04ae74abf0f620189f1dd6cbb2586f8f6267e42268f7992e2f9dfaa67e9b323c";
      };
      "x86_64-darwin" = {
        file = "pnpm-darwin-x64.tar.gz";
        sha256 = "7fd809ea70a20e0911da456a533731deb530dbde7f3c97a030c97da143ed7466";
      };
      "aarch64-darwin" = {
        file = "pnpm-darwin-arm64.tar.gz";
        sha256 = "c31f5abe796dfc8489ea980cb438844053ad6b402bb4f3eb9abd71086279d75b";
      };
    };

    systems = builtins.attrNames assets;
    forAllSystems = f: nixpkgs.lib.genAttrs systems f;

    pkgsFor = system:
      if system == "x86_64-darwin"
      then nixpkgs-darwin-legacy.legacyPackages.${system}
      else nixpkgs.legacyPackages.${system};

    # nixpkgs with rust-overlay applied, for the from-source build.
    # nixpkgs's packaged Rust may lag behind the MSRV the workspace
    # requires (rust-toolchain.toml pins 1.97.0; nixpkgs-unstable
    # ships 1.95.0), so we overlay the exact toolchain.
    rustPkgsFor = system:
      import (if system == "x86_64-darwin" then nixpkgs-darwin-legacy else nixpkgs) {
        inherit system;
        overlays = [ rust-overlay.overlays.default ];
      };

    # Rust toolchain matching rust-toolchain.toml.
    rustToolchainFor = system:
      (rustPkgsFor system).rust-bin.fromRustupToolchainFile "${self}/rust-toolchain.toml";

    # Prebuilt standalone binary from GitHub Releases. The v12 release
    # ships the Rust pnpm-cli binary (built via cross in the release
    # workflow) staged beside a dist/ directory containing node-gyp
    # support files. The binary resolves dist/ from
    # dirname(process.execPath), so the install phase preserves the
    # tarball's sibling layout under libexec and symlinks the binary
    # into bin.
    pnpmFor = system: let
      pkgs = pkgsFor system;
      asset = assets.${system};
    in pkgs.stdenv.mkDerivation {
      pname = "pnpm";
      inherit version;

      src = pkgs.fetchurl {
        url = "https://github.com/pnpm/pnpm/releases/download/v${version}/${asset.file}";
        inherit (asset) sha256;
      };

      sourceRoot = ".";

      nativeBuildInputs = pkgs.lib.optionals pkgs.stdenv.isLinux [ pkgs.autoPatchelfHook ];
      buildInputs = pkgs.lib.optionals pkgs.stdenv.isLinux [ pkgs.stdenv.cc.cc.lib ];

      dontConfigure = true;
      dontBuild = true;

      installPhase = ''
        runHook preInstall
        mkdir -p "$out/libexec/pnpm" "$out/bin"
        cp pnpm "$out/libexec/pnpm/pnpm"
        cp -r dist "$out/libexec/pnpm/dist"
        chmod +x "$out/libexec/pnpm/pnpm"
        ln -s "$out/libexec/pnpm/pnpm" "$out/bin/pnpm"
        runHook postInstall
      '';

      meta = with pkgs.lib; {
        description = "Fast, disk space efficient package manager";
        homepage = "https://github.com/pnpm/pnpm";
        downloadPage = "https://github.com/pnpm/pnpm/releases";
        license = licenses.mit;
        mainProgram = "pnpm";
        platforms = systems;
        sourceProvenance = [ sourceTypes.binaryNativeCode ];
      };
    };

    # From-source build of the Rust pnpm CLI (pacquet). The Cargo
    # workspace root is at the repo root with members in
    # pnpm/crates/*, pnpm/tasks/*, and pnpr/crates/*. We build only
    # the pnpm-cli crate's `pnpm` binary. The runtime dist/ directory
    # (node-gyp support files) is fetched from the release tarball
    # because reproducing it requires the full TypeScript build
    # pipeline; the Rust binary itself is built entirely from source.
    pnpmSourceFor = system: let
      pkgs = rustPkgsFor system;
      craneLib = crane.mkLib pkgs;
      rustToolchain = rustToolchainFor system;

      # Filter to only Rust-related files so TypeScript/JS changes
      # don't invalidate the Crane dependency cache. Include .inc and
      # .json files since some crates use include_str! on them.
      rustSrc = pkgs.lib.cleanSourceWith {
        src = self;
        filter = path: type:
          (craneLib.filterCargoSources path type)
          || (builtins.match ''.*rust-toolchain.*'' path != null)
          || (pkgs.lib.hasSuffix ".inc" path)
          || (pkgs.lib.hasSuffix ".json" path);
      };

      # Vendor all workspace dependencies from Cargo.lock using the
      # toolchain pinned by rust-toolchain.toml. The CARGO and RUSTC
      # env vars force crane's hooks to use the overlay toolchain
      # instead of nixpkgs's packaged Rust (which may be older).
      cargoArtifacts = craneLib.buildDepsOnly {
        src = rustSrc;
        pname = "pnpm-cargo-deps";
        version = version;
        rustToolchain = rustToolchain;
        cargoExtraArgs = "-p pnpm-cli";
        # Skip tests and limit check to lib/bins so dev-dependencies
        # (pnpm-testing-utils → pnpr → libsql-ffi) are not compiled.
        # libsql-ffi's bundled SQLite build script fails under the
        # Nix sandbox (cp into OUT_DIR denied).
        doCheck = false;
        cargoCheckExtraArgs = "--lib --bins";
        CARGO = "${rustToolchain}/bin/cargo";
        RUSTC = "${rustToolchain}/bin/rustc";
      };

      asset = assets.${system};

      # The dist/ directory (node-gyp support) from the release
      # tarball. The Rust binary expects this beside itself at
      # runtime for native addon compilation support.
      distTarball = pkgs.fetchurl {
        url = "https://github.com/pnpm/pnpm/releases/download/v${version}/${asset.file}";
        inherit (asset) sha256;
      };
    in craneLib.buildPackage {
      pname = "pnpm";
      inherit version;
      src = rustSrc;
      inherit cargoArtifacts rustToolchain;

      # Build only the pnpm-cli binary, not the entire workspace.
      cargoExtraArgs = "-p pnpm-cli --bin pnpm";

      # Force crane's hooks to use the overlay toolchain.
      CARGO = "${rustToolchain}/bin/cargo";
      RUSTC = "${rustToolchain}/bin/rustc";

      # No tests in this phase — the workspace's test suite is large
      # and runs in the project's own CI, not in the Nix build.
      doCheck = false;

      postInstall = ''
        mkdir -p "$out/libexec/pnpm"
        mv "$out/bin/pnpm" "$out/libexec/pnpm/pnpm"
        # Extract dist/ from the release tarball for node-gyp runtime
        # support. The binary loads it from dirname(process.execPath).
        tar xzf "${distTarball}" -C "$out/libexec/pnpm" --strip-components=0 dist
        ln -s "$out/libexec/pnpm/pnpm" "$out/bin/pnpm"
      '';

      meta = with pkgs.lib; {
        description = "Fast, disk space efficient package manager (Rust, from source)";
        homepage = "https://github.com/pnpm/pnpm";
        license = licenses.mit;
        mainProgram = "pnpm";
        platforms = systems;
        sourceProvenance = [ sourceTypes.fromSource ];
      };
    };

    # nixpkgs-packaged pnpm (JS bundle on system Node.js, with shell
    # completions generated via postInstall). This gives users access
    # to the nixpkgs packaging with its integration hooks.
    nixpkgsPnpmFor = system:
      (if system == "x86_64-darwin"
       then nixpkgs-darwin-legacy.legacyPackages.${system}
       else nixpkgs.legacyPackages.${system}).pnpm;
  in {
    packages = forAllSystems (system: rec {
      pnpm = pnpmFor system;
      prebuilt = pnpm;
      source = pnpmSourceFor system;
      default = prebuilt;
      nixpkgs = nixpkgsPnpmFor system;
    });

    apps = forAllSystems (system: let
      pnpmPkg = pnpmFor system;
      sourcePkg = pnpmSourceFor system;
      nixpkgsPkg = nixpkgsPnpmFor system;
    in {
      pnpm = {
        type = "app";
        program = "${pnpmPkg}/bin/pnpm";
      };
      prebuilt = {
        type = "app";
        program = "${pnpmPkg}/bin/pnpm";
      };
      source = {
        type = "app";
        program = "${sourcePkg}/bin/pnpm";
      };
      default = {
        type = "app";
        program = "${pnpmPkg}/bin/pnpm";
      };
      nixpkgs = {
        type = "app";
        program = "${nixpkgsPkg}/bin/pnpm";
      };
    });

    checks = forAllSystems (system: {
      prebuilt = pnpmFor system;
      source = pnpmSourceFor system;
      nixpkgs = nixpkgsPnpmFor system;
    });
  };
}
