{
  description = "microhop - Minimal initramfs /init binary and generator";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    {
      nixConfig = {
        extra-substituters = [ "https://mobile-nixos-next.cachix.org" ];
        extra-trusted-public-keys = [ "mobile-nixos-next.cachix.org-1:tPehb3T4X8DKn3sVsOUu010Tw8MFElOSizi2w3AQc5Y=" ];
      };
    } // flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};

        bootComponents = if system == "aarch64-linux" then {
          kernel = import ./nixos/kernel.nix { inherit pkgs; };
          u-boot = import ./nixos/u-boot.nix { inherit pkgs; };

          microhopConfig = import ./nixos/microhop-config.nix { inherit pkgs; };

          nixos-rootfs = import ./nixos/nixos-rootfs.nix {
            inherit pkgs nixpkgs;
          };
        } else {};
      in
      {
        packages = {
          microhop = pkgs.pkgsStatic.rustPlatform.buildRustPackage rec {
            pname = "microhop";
            version = "0.1.0";

            src = ./.;

            cargoLock = {
              lockFile = ./Cargo.lock;
              outputHashes = {
                "kmoddep-0.1.5" = "sha256-6ZKyDKJKisE6Ky+L33Di8Yfv986SbS+WoOC2grS497s=";
              };
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
              outputHashes = {
                "kmoddep-0.1.5" = "sha256-6ZKyDKJKisE6Ky+L33Di8Yfv986SbS+WoOC2grS497s=";
              };
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

            meta = with pkgs.lib; {
              description = "Initramfs generator tool for microhop";
              homepage = "https://github.com/tinythings/microhop";
              license = licenses.asl20;
              maintainers = [];
              platforms = [ "aarch64-linux" "x86_64-linux" ];
            };
          };

          default = self.packages.${system}.microgen;
        } // (if system == "aarch64-linux" then {

          kernel = bootComponents.kernel;
          u-boot = bootComponents.u-boot;
          nixos-rootfs = bootComponents.nixos-rootfs;

          initramfs-microgen = import ./nixos/initramfs-microgen.nix {
            inherit pkgs;
            kernel = bootComponents.kernel;
            microgen = self.packages.${system}.microgen;
            microhop = self.packages.${system}.microhop;
            microhopConfig = bootComponents.microhopConfig;
          };

          boot-overlayfs-musl = pkgs.writeScriptBin "boot-qemu-overlayfs-musl" ''
            #!${pkgs.bash}/bin/bash

            UBOOT="${bootComponents.u-boot}/u-boot.bin"
            KERNEL="${bootComponents.kernel}/Image"
            INITRD="${self.packages.${system}.initramfs-microgen}/initrd"
            SQUASHFS="${bootComponents.nixos-rootfs}/rootfs.squashfs"

            OVERLAY_IMG=$(${pkgs.coreutils}/bin/mktemp -u /tmp/overlay-XXXXXX.img)
            trap 'rm -f "$OVERLAY_IMG"' EXIT
            
            ${pkgs.coreutils}/bin/truncate -s 512M "$OVERLAY_IMG"
            ${pkgs.e2fsprogs}/bin/mkfs.ext4 -F -L overlay-storage "$OVERLAY_IMG"
            
            echo "=========================================="
            echo "  Microhop Boot Flow"
            echo "=========================================="
            echo "Booting with musl-based rootfs and overlayfs..."
            echo ""
            echo "U-Boot:         $UBOOT"
            echo "Kernel:         $KERNEL"
            echo "Initrd:         $INITRD"
            echo "Rootfs:         $SQUASHFS"
            echo "Overlay:        $OVERLAY_IMG"
            echo ""
            echo "Boot flow: U-Boot -> Kernel -> Microhop initramfs -> NixOS musl rootfs"
            echo "=========================================="
            echo ""

            ${pkgs.qemu}/bin/qemu-system-aarch64 \
              -M virt \
              -cpu cortex-a57 \
              -m 1024M \
              -smp 2 \
              -bios "$UBOOT" \
              -kernel "$KERNEL" \
              -initrd "$INITRD" \
              -drive file="$SQUASHFS",if=none,format=raw,readonly=on,id=hd0 \
              -device virtio-blk-device,drive=hd0 \
              -drive file="$OVERLAY_IMG",if=none,format=raw,id=hd1 \
              -device virtio-blk-device,drive=hd1 \
              -append "console=ttyAMA0" \
              -nographic \
              -no-reboot

            rm -f "$OVERLAY_IMG"
          '';
        } else {});

        apps = if system == "aarch64-linux" then {
          boot-overlayfs-musl = {
            type = "app";
            program = "${self.packages.${system}.boot-overlayfs-musl}/bin/boot-qemu-overlayfs-musl";
          };
          default = {
            type = "app";
            program = "${self.packages.${system}.boot-overlayfs-musl}/bin/boot-qemu-overlayfs-musl";
          };
        } else {};


        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            gnumake
            cargo
            rustc
            rustfmt
            clippy
            pkg-config
            util-linux
            llvmPackages_20.libclang
            llvmPackages_20.clang
          ];

          shellHook = ''
            export LIBCLANG_PATH=${pkgs.libclang.lib}/lib
            export BINDGEN_EXTRA_CLANG_ARGS="--sysroot=${pkgs.glibc.dev} -I${pkgs.util-linux.dev}/include"
          '';
        };
      }
    );
}
