{
  description = "A files database for nixpkgs";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    flake-compat = {
      url = "github:edolstra/flake-compat";
      flake = false;
    };

    crane.url = "github:ipetkov/crane";

    flake-utils = {
      url = "github:numtide/flake-utils";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      crane,
      ...
    }:
    let
      inherit (nixpkgs) lib;
      systems = [
        "x86_64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
        "aarch64-linux"
      ];
      nixpkgsFor = system: nixpkgs.legacyPackages.${system};


      cargoTOML = lib.importTOML ./Cargo.toml;
    in
    flake-utils.lib.eachSystem systems (
      system:
      let
        pkgs = nixpkgsFor system;
        craneLib = crane.mkLib pkgs;

        includeSourceFiles = [
          "command-not-found.sh"
          "command-not-found.nu"
        ];

        filterSources = path: type:
          (craneLib.filterCargoSources path type) || 
          (lib.elem (lib.baseNameOf path) includeSourceFiles && type == "regular");

        commonArgs = {
          src = lib.cleanSourceWith {
            filter = filterSources;
            src = ./.;
          };
          strictDeps = true;

          nativeBuildInputs = with pkgs; [
            pkg-config
            rustPlatform.bindgenHook
          ];

          buildInputs = with pkgs; [
            sqlite
            nix.dev
          ];
        };

        nix-index = craneLib.buildPackage (commonArgs // {
            pname = "nix-index";
            inherit (cargoTOML.package) version;

            cargoArtifacts = craneLib.buildDepsOnly commonArgs;
            
            postInstall = ''
              substituteInPlace command-not-found.sh \
                --subst-var out
              install -Dm555 command-not-found.sh -t $out/etc/profile.d
              substituteInPlace command-not-found.nu \
                --subst-var out
              install -Dm555 command-not-found.nu -t $out/etc/profile.d
            '';

            meta = with lib; {
              description = "A files database for nixpkgs";
              homepage = "https://github.com/nix-community/nix-index";
              license = with licenses; [ bsd3 ];
              maintainers = [ maintainers.bennofs ];
              mainProgram = "nix-index";
            };
          });
      in
      {
        packages.default = nix-index;

        checks = {
          inherit nix-index;
        };

        devShells = {
          default = craneLib.devShell {
            checks = self.checks.${system};
            name = "nix-index";

            packages = with pkgs; [
              nixfmt nixd
            ];
          };
        };

        apps = {
          nix-index = {
            type = "app";
            program = lib.getExe' "nix-index" self.packages.${system}.default;
          };

          nix-locate = {
            type = "app";
            program = lib.getExe' "nix-locate" self.packages.${system}.default;
          };

          default = self.apps.${system}.nix-locate;
        };
      }
    );
}
