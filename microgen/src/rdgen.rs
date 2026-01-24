use kmoddep::kerman::KernelInfo;
use profile::cfg::MhConfig;
use std::{
    collections::HashSet,
    env,
    fs::{self, File},
    io::{BufWriter, Error, ErrorKind::InvalidData, Write},
    os::unix::fs::{symlink, PermissionsExt},
    path::{Path, PathBuf},
};

use crate::rdpack;

const MICROHOP: &[u8] = include_bytes!("microhop");
const BLINKENLICHTEN: &str = "# Achtung Alles Lookenskepers!
#
# Das konfiguration ist nicht fuer gefingerpoken und
# mittengrabben. Ist easy das machine schnappen der springenwerk,
# blowenfusen und poppencorken mit spitzensparken. Das rubbernecken
# sichtseeren keepen das cotten-pickenen hands in das pockets
# muss.
#
# Relaxen und watchen das blinkenlichten.";

pub struct IrfsGen {
    /// Target kernel (optional: monolithic kernel has none)
    kinfo: Option<KernelInfo>,

    /// Profile (config)
    cfg: MhConfig,

    /// Destination where initramfs is going to be generated
    dst: PathBuf,

    /// Output filename path
    dst_fn: PathBuf,

    /// Module dependencies
    _kmod_d: Vec<String>,

    /// Main modules
    _kmod_m: Vec<String>,
}

impl IrfsGen {
    pub fn generate(kinfo: Option<&KernelInfo>, cfg: MhConfig, dst: PathBuf, fname: PathBuf) -> Result<(), Error> {
        if dst.exists() {
            return Err(Error::new(InvalidData, format!("Given destination path {:?} already exists", dst)));
        }

        fs::create_dir_all(&dst)?;

        let mut irfsg = IrfsGen { kinfo: kinfo.cloned(), cfg, dst, dst_fn: fname, _kmod_d: vec![], _kmod_m: vec![] };

        let kroot = irfsg.create_ramfs_dirs()?;
        irfsg.setup_microhop()?;

        if irfsg.kinfo.is_some() {
            irfsg.copy_kernel_modules(kroot.as_str())?;
        } else {
            println!("ℹ No kernel modules found, skipping module copy");
        }

        irfsg.write_boot_config()?;
        irfsg.pack()?;

        Ok(())
    }

    /// Copy microhop binary
    fn setup_microhop(&self) -> Result<(), Error> {
        let mhp = self.dst.join("bin/microhop");
        fs::write(&mhp, MICROHOP)?;
        let mut flags = fs::metadata(&mhp)?.permissions();
        flags.set_mode(0o755);
        fs::set_permissions(mhp, flags)?;

        // Symlink to /init
        let here = env::current_dir()?;
        env::set_current_dir(self.dst.as_path())?;
        symlink(Path::new("bin/microhop"), Path::new("init"))?;
        env::set_current_dir(here)?;

        Ok(())
    }

    /// Create directories for the ramfs.
    fn create_ramfs_dirs(&self) -> Result<String, Error> {
        let mut dirs: Vec<String> = vec![
            "bin".to_string(),
            "etc".to_string(),
            "proc".to_string(),
            "dev".to_string(),
            "sys".to_string(),
            self.cfg.get_sysroot_path().trim_start_matches('/').to_string(),
        ];

        let kroot = if let Some(kinfo) = &self.kinfo {
            let kr = format!("lib/modules/{}", kinfo.get_kernel_path().file_name().unwrap().to_str().unwrap());
            dirs.push(kr.clone());
            kr
        } else {
            String::new()
        };

        for d in dirs {
            fs::create_dir_all(self.dst.join(d))?;
        }

        Ok(kroot)
    }

    /// This will find what modules are needed in the source kernel and will copy to the target only those
    fn copy_kernel_modules(&mut self, kroot: &str) -> Result<(), Error> {
        let kinfo = match &self.kinfo {
            Some(k) => k,
            None => return Ok(()),
        };

        // First get only main modules, and then get dependencies for them
        for (kmod, kmod_deps) in kinfo.get_deps_for(
            &self
                .cfg
                .get_modules()
                .iter()
                .filter(|e| !kinfo.is_dep(e))
                .collect::<HashSet<_>>()
                .into_iter()
                .cloned()
                .collect::<Vec<String>>(),
        ) {
            self._copy_kmod(&kmod, kroot)?;
            self._kmod_m.push(kmod);
            if !kmod_deps.is_empty() {
                for kd in kmod_deps {
                    self._copy_kmod(kd.as_str(), kroot)?;
                    if !self._kmod_m.contains(&kd) {
                        self._kmod_d.push(kd);
                    }
                }
            }
        }
        Ok(())
    }

    /// Copy one kernel module
    fn _copy_kmod(&self, kmod: &str, kroot: &str) -> Result<(), Error> {
        let kinfo = match &self.kinfo {
            Some(k) => k,
            None => return Ok(()),
        };

        let msrc = kinfo.get_kernel_path().join(kmod);
        let mdst = self.dst.join(kroot).join(kmod);

        fs::create_dir_all(mdst.as_path().parent().unwrap())?;
        fs::copy(msrc, mdst)?;

        Ok(())
    }

    /// Write boot config
    fn write_boot_config(&self) -> Result<(), Error> {
        let f = File::create(self.dst.join("etc/microhop.conf"))?;
        let mut fp = BufWriter::new(f);

        // Blinkenlichten :)
        writeln!(fp, "{}\n", BLINKENLICHTEN)?;

        // Write modules in the following order:
        //   1. First dependencies
        //   2. Main modules
        writeln!(
            fp,
            "modules:\n{}\n",
            self._kmod_d
                .iter()
                .chain(self._kmod_m.iter())
                .map(|i| format!("  - {}", Path::new(i).file_stem().unwrap().to_str().unwrap().split('.').next().unwrap()))
                .collect::<Vec<String>>()
                .join("\n")
        )?;

        // Write disks configuration
        writeln!(fp, "disks:")?;
        for d in self.cfg.get_disks()? {
            writeln!(fp, "  {}: {},{},{}", d.get_device(), d.get_fstype(), d.get_mountpoint(), d.get_mode())?;
        }
        writeln!(fp)?;

        // Transfer other options
        writeln!(fp, "init: {}", self.cfg.get_init_path())?;
        writeln!(fp, "sysroot: {}", self.cfg.get_sysroot_path())?;

        if let Some(l) = self.cfg.get_log_level_as_str() {
            writeln!(fp, "log: {}", l)?;
        }

        fp.flush()?;
        Ok(())
    }

    /// Pack to the CPIO
    fn pack(&self) -> Result<(), Error> {
        let here = env::current_dir()?;
        env::set_current_dir(self.dst.as_path())?;

        let out = self.dst_fn.as_os_str().to_str().unwrap();
        println!("Writing the initramfs to {:?}", out);
        rdpack::pack(out)?;

        env::set_current_dir(here)?;
        fs::remove_dir_all(&self.dst)?;

        println!("Done");
        Ok(())
    }
}
