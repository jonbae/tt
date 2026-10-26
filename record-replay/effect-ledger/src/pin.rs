//! Pinning: the ledger carries the binary that recorded it, and replay only
//! ever runs that binary.

use std::fs;
#[cfg(target_os = "linux")]
use std::fs::File;
#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::die;

/// clone, then hardlink, then copy (rr's order).
pub fn pin(src: &Path, dst: &Path) -> &'static str {
    #[cfg(target_os = "macos")]
    {
        let s = std::ffi::CString::new(src.as_os_str().as_bytes()).unwrap();
        let d = std::ffi::CString::new(dst.as_os_str().as_bytes()).unwrap();
        if unsafe { libc::clonefile(s.as_ptr(), d.as_ptr(), 0) } == 0 {
            return "clone";
        }
    }
    #[cfg(target_os = "linux")]
    {
        const FICLONE: libc::c_ulong = 0x40049409;
        if let (Ok(from), Ok(to)) = (File::open(src), File::create(dst)) {
            if unsafe { libc::ioctl(to.as_raw_fd(), FICLONE, from.as_raw_fd()) } == 0 {
                let _ = fs::set_permissions(dst, fs::metadata(src).unwrap().permissions());
                return "clone";
            }
            let _ = fs::remove_file(dst);
        }
    }
    if fs::hard_link(src, dst).is_ok() {
        return "hardlink";
    }
    fs::copy(src, dst).unwrap_or_else(|e| die(74, &format!("cannot pin binary: {e}")));
    "copy"
}

// MVP: pin each build once into `<ledger>/.pins/`, and hardlink every
// instance's exe to that copy. macOS charges ~280 ms the first time any new file
// is executed, so a fresh clone per request made recording ~50x slower. This
// pays it once per build. The shared copy is a clone (or copy) of the original,
// never a hardlink to it, so overwriting the original in place can't reach it.
pub fn pin_shared(src: &Path, dst: &Path) -> &'static str {
    use std::os::unix::fs::MetadataExt;
    let Some(store) = dst.parent().and_then(Path::parent).map(|ledgers| ledgers.join(".pins")) else {
        return pin(src, dst);
    };
    // The same file with the same contents: device, inode, size, mtime (ns).
    let m = fs::metadata(src).unwrap();
    let shared = store.join(format!("{:x}-{:x}-{:x}-{:x}.{:x}", m.dev(), m.ino(), m.size(), m.mtime(), m.mtime_nsec()));
    if !shared.exists() {
        let _ = fs::create_dir_all(&store);
        let tmp = store.join(format!(".tmp-{}", std::process::id()));
        let _ = fs::remove_file(&tmp);
        pin(src, &tmp);
        // A concurrent recorder may win the rename; both copies are the same build.
        let _ = fs::rename(&tmp, &shared);
    }
    if fs::hard_link(&shared, dst).is_ok() {
        return "shared";
    }
    pin(src, dst)
}

pub fn current_exe() -> PathBuf {
    #[cfg(target_os = "linux")]
    return PathBuf::from("/proc/self/exe");
    #[cfg(not(target_os = "linux"))]
    std::env::current_exe().unwrap()
}
