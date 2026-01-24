{
  description = "microhop - Minimal initramfs /init binary and generator";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in
      {
        packages = {
          microhop = pkgs.pkgsStatic.rustPlatform.buildRustPackage rec {
            pname = "microhop";
            version = "0.1.0";

            src = ./.;

            cargoLock = {
              lockFile = ./Cargo.lock;
            };

            nativeBuildInputs = with pkgs.pkgsStatic; [
              pkg-config
              rustPlatform.bindgenHook
            ];

            buildInputs = with pkgs.pkgsStatic; [
              util-linuxMinimal
            ];

            buildType = "release";

            cargoBuildFlags = [ "-p" "microhop" ];

            doCheck = false;

            stripAllList = [ "bin" ];

            meta = with pkgs.lib; {
              description = "Minimal initramfs /init binary";
              homepage = "https://github.com/tinythings/microhop";
              license = licenses.asl20;
              maintainers = [];
              platforms = [ "aarch64-linux" "x86_64-linux" ];
            };
          };

          microgen = pkgs.pkgsStatic.rustPlatform.buildRustPackage rec {
            pname = "microgen";
            version = "0.1.0";

            src = ./.;

            cargoLock = {
              lockFile = ./Cargo.lock;
            };

            nativeBuildInputs = with pkgs.pkgsStatic; [
              pkg-config
              rustPlatform.bindgenHook
            ];

            buildInputs = with pkgs.pkgsStatic; [
              util-linuxMinimal
            ];

            buildType = "release";

            cargoBuildFlags = [ "-p" "microgen" ];

            doCheck = false;

            preBuild = ''
              cp ${self.packages.${system}.microhop}/bin/microhop microgen/src/microhop
            '';

            meta = with pkgs.lib; {
              description = "Initramfs generator tool for microhop";
              homepage = "https://github.com/tinythings/microhop";
              license = licenses.asl20;
              maintainers = [];
              platforms = [ "aarch64-linux" "x86_64-linux" ];
            };
          };

          default = self.packages.${system}.microgen;
        };

        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            make
            cargo
            rustc
            rustfmt
            clippy
            pkg-config
            util-linux
          ];
        };
      }
    );
}
