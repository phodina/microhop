{ pkgs, nixpkgs }:

let
  pkgsMusl = import nixpkgs {
    localSystem = { system = "aarch64-linux"; };
    crossSystem = {
      config = "aarch64-unknown-linux-musl";
      isStatic = false;
    };
  };

  muslBash = pkgsMusl.bash;
  muslCoreutils = pkgsMusl.coreutils;
  muslUtilLinux = pkgsMusl.util-linux;
  muslBusybox = pkgsMusl.busybox;

  initScript = pkgsMusl.writeScript "init" ''
    #!${muslBash}/bin/bash
    set -e

    echo "==========================================="
    echo "  Nixos rootfs Init for 'microhop'"
    echo "  Architecture: $(${muslCoreutils}/bin/uname -m)"
    echo "==========================================="
    echo ""

    ${muslUtilLinux}/bin/mount -t proc proc /proc
    ${muslUtilLinux}/bin/mount -t sysfs sysfs /sys
    ${muslUtilLinux}/bin/mount -t devtmpfs devtmpfs /dev
    
    echo "System initialized successfully!"
    echo ""

    # Set up environment variables
    export PATH="/bin:/sbin:/usr/bin:/usr/sbin:${muslBash}/bin:${muslCoreutils}/bin:${muslUtilLinux}/bin:${muslBusybox}/bin"
    export HOME="/root"
    export TERM="linux"

    echo "Starting interactive shell..."
    exec ${muslBash}/bin/bash
  '';

  closure = pkgsMusl.closureInfo {
    rootPaths = [ muslBash muslCoreutils muslUtilLinux muslBusybox initScript ];
  };

in pkgsMusl.runCommand "nixos-rootfs-musl" {
  nativeBuildInputs = with pkgs; [ squashfsTools ];
} ''
  mkdir -p $out
  mkdir -p $TMPDIR/rootfs

  mkdir -p $TMPDIR/rootfs/{dev,proc,sys,run,tmp,var,home,root,etc,bin,sbin,usr/bin,usr/sbin}
  chmod 1777 $TMPDIR/rootfs/tmp

  mkdir -p $TMPDIR/rootfs/nix/store
  for storePath in $(< ${closure}/store-paths); do
    echo "  $storePath"
    cp -a $storePath $TMPDIR/rootfs/nix/store/
  done

  ln -s ${initScript} $TMPDIR/rootfs/init
  ln -s ${initScript} $TMPDIR/rootfs/sbin/init

  ln -s ${muslBash}/bin/bash $TMPDIR/rootfs/bin/bash
  ln -s ${muslBash}/bin/sh $TMPDIR/rootfs/bin/sh
  ln -s ${muslCoreutils}/bin/uname $TMPDIR/rootfs/bin/uname
  ln -s ${muslCoreutils}/bin/ls $TMPDIR/rootfs/bin/ls
  ln -s ${muslCoreutils}/bin/cat $TMPDIR/rootfs/bin/cat
  ln -s ${muslUtilLinux}/bin/mount $TMPDIR/rootfs/bin/mount
  ln -s ${muslUtilLinux}/bin/umount $TMPDIR/rootfs/bin/umount

  ln -s ${muslUtilLinux}/bin/mount $TMPDIR/rootfs/sbin/mount
  ln -s ${muslUtilLinux}/bin/umount $TMPDIR/rootfs/sbin/umount

  echo "root:x:0:0:root:/root:/bin/bash" > $TMPDIR/rootfs/etc/passwd
  echo "root:x:0:" > $TMPDIR/rootfs/etc/group
  echo "localhost" > $TMPDIR/rootfs/etc/hostname

  ${pkgs.squashfsTools}/bin/mksquashfs $TMPDIR/rootfs $out/rootfs.squashfs \
    -comp zstd \
    -Xcompression-level 15 \
    -noappend \
    -no-progress
''
