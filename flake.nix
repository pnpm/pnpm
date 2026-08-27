{
  description = "Fast, disk space efficient package manager";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    # Pin x86_64-darwin to a stable darwin branch. nixpkgs-unstable
    # (26.11) dropped x86_64-darwin support; the -darwin branch
    # receives security updates without the breaking churn.
    nixpkgs-darwin-legacy.url = "github:NixOS/nixpkgs/nixpkgs-26.05-darwin";
  };

  outputs = { self, nixpkgs, nixpkgs-darwin-legacy }: let
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

    # Prebuilt standalone binary (Node.js SEA with embedded JS bundle).
    # The binary loads dist/node_modules and dist/node-gyp-bin from
    # dirname(process.execPath), so the install phase preserves the
    # tarball's sibling layout under libexec and symlinks the binary
    # into bin.
    pnpmFor = system: let
      pkgs =
        if system == "x86_64-darwin"
        then nixpkgs-darwin-legacy.legacyPackages.${system}
        else nixpkgs.legacyPackages.${system};
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

    # nixpkgs-packaged pnpm (JS bundle on system Node.js, with shell
    # completions generated via postInstall). This gives users access
    # to the nixpkgs packaging with its integration hooks. The prebuilt
    # output above is the standalone binary with embedded Node.js.
    nixpkgsPnpmFor = system:
      (if system == "x86_64-darwin"
       then nixpkgs-darwin-legacy.legacyPackages.${system}
       else nixpkgs.legacyPackages.${system}).pnpm;
  in {
    packages = forAllSystems (system: rec {
      pnpm = pnpmFor system;
      prebuilt = pnpm;
      default = prebuilt;
      nixpkgs = nixpkgsPnpmFor system;
    });

    apps = forAllSystems (system: let
      pnpmPkg = pnpmFor system;
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
      nixpkgs = nixpkgsPnpmFor system;
    });
  };
}
