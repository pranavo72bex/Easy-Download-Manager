use super::*;

/// Disk space promised to the downloads under way. Each download that
/// passes the free-space check claims what it still needs; the next one is
/// checked against the free space less what the others still need on the
/// same disk. Without it, downloads started together each saw the whole
/// free space and together could still fill the disk.
/// A part's claim shrinks as its bytes arrive (they are then counted by the
/// file system's own free space instead); the finished file's claim on
/// another disk is whole until the download ends.
#[derive(Clone, Default)]
pub struct Room {
    inner: Arc<std::sync::Mutex<Claims>>,
    /// Where the free space is read; `free_space` unless a test says.
    free: Option<Arc<FreeSpace>>,
    /// Where the largest file a folder's drive holds is read;
    /// `largest_file` unless a test says.
    largest: Option<Arc<FreeSpace>>,
}

/// A reader of a folder's free space.
pub(super) type FreeSpace = dyn Fn(&Path) -> Option<u64> + Send + Sync;

#[derive(Default)]
pub(super) struct Claims {
    next: u64,
    held: std::collections::HashMap<u64, Claim>,
}

pub(super) struct Claim {
    dir: PathBuf,
    size: u64,
    /// The download's bytes so far, for a part's claim; `None` for a
    /// finished file's, which is whole until the end.
    done: Option<Arc<AtomicU64>>,
}

impl Claim {
    pub(super) fn outstanding(&self) -> u64 {
        match &self.done {
            Some(done) => self.size.saturating_sub(done.load(Ordering::Relaxed)),
            None => self.size,
        }
    }
}

impl std::fmt::Debug for Room {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Room").finish_non_exhaustive()
    }
}

impl Room {
    /// A ledger that reads free space through `free`: for tests, a disk of
    /// any size.
    pub fn with_free_space(free: impl Fn(&Path) -> Option<u64> + Send + Sync + 'static) -> Self {
        Room {
            inner: Arc::default(),
            free: Some(Arc::new(free)),
            largest: None,
        }
    }

    /// A ledger whose drives hold no file over what `largest` says: for
    /// tests, a FAT32 drive.
    pub fn with_largest_file(
        largest: impl Fn(&Path) -> Option<u64> + Send + Sync + 'static,
    ) -> Self {
        Room {
            largest: Some(Arc::new(largest)),
            ..Room::default()
        }
    }

    pub(super) fn largest_file(&self, dir: &Path) -> Option<u64> {
        match &self.largest {
            Some(largest) => largest(dir),
            None => largest_file(dir),
        }
    }

    pub(super) fn free_space(&self, dir: &Path) -> Option<u64> {
        match &self.free {
            Some(free) => free(dir),
            None => free_space(dir),
        }
    }
}

/// What one download has claimed, given back when it ends however it ends.
pub struct Claimed {
    room: Room,
    ids: Vec<u64>,
}

impl Drop for Claimed {
    fn drop(&mut self) {
        let mut claims = crate::lock(&self.room.inner);
        for id in &self.ids {
            claims.held.remove(id);
        }
    }
}

/// Refuses a download the disk cannot hold, before a byte of it is fetched. Without it a stated 100 TB ran until the disk was full,
/// taking every other program's room with it. The parts need what is not
/// yet staged; the finished file needs its whole length where it lands,
/// when that is another disk (on the same one, the staged file is renamed
/// into place). What other downloads under way still need on the same disk is
/// taken from the free space first (`Room`). Where the free space cannot be
/// read, nothing is refused. The claim lasts as long as what is returned.
pub(super) fn room_for(
    size: u64,
    done: &Arc<AtomicU64>,
    options: &Options,
) -> Result<Claimed, Error> {
    let room = &options.room;
    let parts = &options.parts_dir;
    let dest = &options.dest_dir;
    let mut needs = vec![(parts, Some(Arc::clone(done)))];
    if !same_disk(parts, dest) {
        needs.push((dest, None));
    }
    // A drive whose file system cannot hold a file this big (FAT32's 4 GB)
    // is said before a byte is fetched: the download ran to 4 GB and failed
    // there.
    for (dir, _) in &needs {
        if let Some(most) = room.largest_file(dir) {
            if size > most {
                return Err(Error::Internal(format!(
                    "too large for this drive: the file is {}, and {} is on a FAT32 drive, which \
                     holds no file over 4 GB; choose a folder on another drive",
                    amount(size),
                    dir.display()
                )));
            }
        }
    }
    // Held across the check and the claim, so two downloads starting at
    // once cannot both pass against the same space.
    let mut claims = crate::lock(&room.inner);
    for (dir, counts) in &needs {
        let need = match counts {
            Some(done) => size.saturating_sub(done.load(Ordering::Relaxed)),
            None => size,
        };
        let Some(free) = room.free_space(dir) else {
            continue;
        };
        let promised: u64 = claims
            .held
            .values()
            .filter(|c| same_disk(&c.dir, dir))
            .map(Claim::outstanding)
            .sum();
        if need > free.saturating_sub(promised) {
            let others = if promised > 0 {
                format!(
                    ", {} of it already needed by downloads under way",
                    amount(promised.min(free))
                )
            } else {
                String::new()
            };
            // The folder the person chose, not the row's own staging
            // folder inside dm's: the download folder
            // where the two share a disk, the chosen parts folder where not.
            let named = if same_disk(parts, dest) {
                dest.as_path()
            } else if dir == &parts {
                parts.parent().unwrap_or(parts)
            } else {
                dir.as_path()
            };
            return Err(Error::Internal(format!(
                "not enough room: the file needs {}, and {} has {} free{others}",
                amount(need),
                named.display(),
                amount(free)
            )));
        }
    }
    let mut ids = Vec::new();
    for (dir, counts) in needs {
        claims.next += 1;
        let id = claims.next;
        claims.held.insert(
            id,
            Claim {
                dir: dir.clone(),
                size,
                done: counts,
            },
        );
        ids.push(id);
    }
    drop(claims);
    Ok(Claimed {
        room: room.clone(),
        ids,
    })
}

/// Bytes as a person reads them.
pub(super) fn amount(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["bytes", "KB", "MB", "GB", "TB", "PB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} bytes")
    } else {
        format!("{value:.1} {}", UNITS.get(unit).unwrap_or(&"bytes"))
    }
}

/// Whether two folders are on one disk. Unix by device number; Windows by
/// drive. Unreadable is taken as one disk, the check that asks less.
pub(super) fn same_disk(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (std::fs::metadata(a), std::fs::metadata(b)) {
            (Ok(a), Ok(b)) => a.dev() == b.dev(),
            _ => true,
        }
    }
    #[cfg(not(unix))]
    {
        let drive = |p: &Path| {
            std::path::absolute(p).ok().and_then(|p| {
                p.components()
                    .next()
                    .map(|c| c.as_os_str().to_ascii_lowercase())
            })
        };
        drive(a) == drive(b)
    }
}

/// The space this account may still write in the folder's file system.
#[cfg(unix)]
pub fn free_space(dir: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::zeroed();
    // SAFETY: `path` is a NUL-terminated string that outlives the call, and
    // `stat` is a buffer of the type statvfs fills; it is read only when the
    // call says it succeeded.
    let done = unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) };
    if done != 0 {
        return None;
    }
    // SAFETY: statvfs returned 0, so it filled the buffer.
    let stat = unsafe { stat.assume_init() };
    // The field types differ between systems; u64 holds each.
    #[allow(clippy::unnecessary_cast)]
    reported_free(
        (stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64),
        (stat.f_blocks as u64).saturating_mul(stat.f_frsize as u64),
    )
}

/// The most a FAT32 file can be: four gigabytes less a byte.
pub(super) const FAT32_MOST: u64 = 4 * 1024 * 1024 * 1024 - 1;

/// The largest file the drive holding `dir` can take, where its file
/// system limits one (FAT32); `None` for no limit dm knows of.
#[cfg(target_os = "linux")]
pub fn largest_file(dir: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
    let mut stat = std::mem::MaybeUninit::<libc::statfs>::zeroed();
    // SAFETY: `path` is NUL-terminated and outlives the call; `stat` is a
    // statfs buffer read only when the call succeeded.
    if unsafe { libc::statfs(path.as_ptr(), stat.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: statfs returned 0, so it filled the buffer.
    let stat = unsafe { stat.assume_init() };
    const MSDOS_SUPER_MAGIC: i64 = 0x4d44;
    #[allow(clippy::unnecessary_cast)]
    (stat.f_type as i64 == MSDOS_SUPER_MAGIC).then_some(FAT32_MOST)
}

/// The largest file the drive holding `dir` can take, where its file
/// system limits one (FAT32, "msdos" to macOS); `None` for no limit.
#[cfg(target_os = "macos")]
pub fn largest_file(dir: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
    let mut stat = std::mem::MaybeUninit::<libc::statfs>::zeroed();
    // SAFETY: as for Linux above.
    if unsafe { libc::statfs(path.as_ptr(), stat.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: statfs returned 0, so it filled the buffer.
    let stat = unsafe { stat.assume_init() };
    // SAFETY: f_fstypename is a NUL-terminated name the call wrote.
    let kind = unsafe { std::ffi::CStr::from_ptr(stat.f_fstypename.as_ptr()) };
    (kind.to_bytes() == b"msdos").then_some(FAT32_MOST)
}

/// The largest file the drive holding `dir` can take, where its file
/// system limits one (FAT32 or FAT); `None` for no limit dm knows of.
#[cfg(windows)]
pub fn largest_file(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{GetVolumeInformationW, GetVolumePathNameW};
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut root = [0u16; 261];
    // SAFETY: `wide` is NUL-terminated; `root` is a buffer of the length
    // given.
    if unsafe { GetVolumePathNameW(wide.as_ptr(), root.as_mut_ptr(), root.len() as u32) } == 0 {
        return None;
    }
    let mut kind = [0u16; 32];
    // SAFETY: `root` holds a NUL-terminated volume path; the out-buffers
    // are the lengths given, and the rest may be null.
    let done = unsafe {
        GetVolumeInformationW(
            root.as_ptr(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            kind.as_mut_ptr(),
            kind.len() as u32,
        )
    };
    if done == 0 {
        return None;
    }
    let end = kind.iter().position(|c| *c == 0).unwrap_or(kind.len());
    let name = String::from_utf16_lossy(kind.get(..end).unwrap_or_default());
    matches!(name.as_str(), "FAT32" | "FAT").then_some(FAT32_MOST)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub fn largest_file(_dir: &Path) -> Option<u64> {
    None
}

/// The free space a file system reported, or unknown when it reported no
/// size at all. Some FUSE and network mounts answer zero free of zero
/// total, and taken at their word every download of known length was
/// refused.
pub(super) fn reported_free(free: u64, total: u64) -> Option<u64> {
    (total > 0).then_some(free)
}

/// The space this account may still write in the folder's volume.
#[cfg(windows)]
pub fn free_space(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
    let (mut free, mut total) = (0u64, 0u64);
    // SAFETY: `wide` is NUL-terminated and outlives the call; the two
    // out-pointers are live u64s and the third may be null.
    let done = unsafe {
        windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free,
            &mut total,
            std::ptr::null_mut(),
        )
    };
    if done == 0 {
        return None;
    }
    reported_free(free, total)
}

#[cfg(not(any(unix, windows)))]
pub fn free_space(_dir: &Path) -> Option<u64> {
    None
}
