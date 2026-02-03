// TODO: Upstream to e2p-sys crate

fn main() {
    // Prefer pkg-config 'e2p' (library providing e2p/e2fsck symbols). Fall back
    // to 'ext2fs' if 'e2p' isn't available.
    let libs = ["e2p", "ext2fs"];
    for name in libs.iter() {
        match pkg_config::probe_library(name) {
            Ok(lib) => {
                for path in lib.link_paths {
                    println!("cargo:rustc-link-search=native={}", path.display());
                }
                for libname in lib.libs {
                    println!("cargo:rustc-link-lib=static={}", libname);
                }
                return;
            }
            Err(_) => continue,
        }
    }

    println!("cargo:warning=Could not find 'e2p' or 'ext2fs' via pkg-config");
}
