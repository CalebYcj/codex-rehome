#[cfg(test)]
mod tests {
    use super::PinnedParent;
    use std::{ffi::OsStr, fs};

    #[test]
    fn pinned_replace_never_writes_through_a_swapped_parent() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("parent");
        let parked = root.path().join("parked");
        let outside = root.path().join("outside");
        let source = root.path().join("source");
        fs::create_dir(&parent).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(parent.join("target"), b"original").unwrap();
        fs::write(outside.join("target"), b"outside").unwrap();
        fs::write(&source, b"replacement").unwrap();
        let pinned = PinnedParent::open(&parent).unwrap();

        let outside_target = swap_parent(&parent, &parked, &outside);
        let result = pinned.replace_file(&source, OsStr::new("target"));

        assert_eq!(fs::read(outside_target).unwrap(), b"outside");
        if result.is_ok() {
            assert_eq!(fs::read(parked.join("target")).unwrap(), b"replacement");
        }
    }

    #[test]
    fn pinned_remove_never_deletes_through_a_swapped_parent() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("parent");
        let parked = root.path().join("parked");
        let outside = root.path().join("outside");
        fs::create_dir(&parent).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(parent.join("target"), b"original").unwrap();
        fs::write(outside.join("target"), b"outside").unwrap();
        let pinned = PinnedParent::open(&parent).unwrap();

        let outside_target = swap_parent(&parent, &parked, &outside);
        let result = pinned.remove_file(OsStr::new("target"));

        assert_eq!(fs::read(outside_target).unwrap(), b"outside");
        if result.is_ok() {
            assert!(!parked.join("target").exists());
        }
    }

    #[cfg(unix)]
    fn swap_parent(
        parent: &std::path::Path,
        parked: &std::path::Path,
        outside: &std::path::Path,
    ) -> std::path::PathBuf {
        use std::os::unix::fs::symlink;
        fs::rename(parent, parked).unwrap();
        symlink(outside, parent).unwrap();
        outside.join("target")
    }

    #[cfg(windows)]
    fn swap_parent(
        parent: &std::path::Path,
        parked: &std::path::Path,
        outside: &std::path::Path,
    ) -> std::path::PathBuf {
        fs::rename(parent, parked).unwrap();
        fs::rename(outside, parent).unwrap();
        parent.join("target")
    }
}
use std::{
    ffi::OsStr,
    fs,
    io::{self, Write},
    path::{Component, Path, PathBuf},
};
use uuid::Uuid;

pub(crate) struct PinnedParent {
    path: PathBuf,
    directory: fs::File,
    identity: DirectoryIdentity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DirectoryIdentity {
    first: u64,
    second: u64,
}

impl PinnedParent {
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() || is_reparse_point(&metadata) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "mutation parent is not a regular directory",
            ));
        }
        let directory = open_directory(path)?;
        let identity = directory_identity(&directory)?;
        let pinned = Self {
            path: path.to_path_buf(),
            directory,
            identity,
        };
        pinned.verify_location()?;
        Ok(pinned)
    }

    pub(crate) fn replace_bytes(&self, name: &OsStr, bytes: &[u8]) -> io::Result<()> {
        self.replace_with(name, |file| file.write_all(bytes))
    }

    pub(crate) fn replace_file(&self, source: &Path, name: &OsStr) -> io::Result<()> {
        let mut source = fs::File::open(source)?;
        self.replace_with(name, |destination| {
            io::copy(&mut source, destination).map(|_| ())
        })
    }

    pub(crate) fn remove_file(&self, name: &OsStr) -> io::Result<()> {
        validate_name(name)?;
        self.verify_location()?;
        remove_at(self, name)?;
        sync_directory_handle(&self.directory)
    }

    pub(crate) fn open_file(&self, name: &OsStr) -> io::Result<fs::File> {
        validate_name(name)?;
        self.verify_location()?;
        open_file_at(self, name)
    }

    pub(crate) fn create_new_file(&self, name: &OsStr) -> io::Result<fs::File> {
        validate_name(name)?;
        self.verify_location()?;
        create_file_at(self, name)
    }

    pub(crate) fn open_file_for_write(&self, name: &OsStr) -> io::Result<fs::File> {
        validate_name(name)?;
        self.verify_location()?;
        open_file_for_write_at(self, name)
    }

    pub(crate) fn sync(&self) -> io::Result<()> {
        sync_directory_handle(&self.directory)
    }

    pub(crate) fn set_permissions(
        &self,
        name: &OsStr,
        permissions: fs::Permissions,
    ) -> io::Result<()> {
        validate_name(name)?;
        self.verify_location()?;
        set_permissions_at(self, name, permissions)
    }

    fn replace_with(
        &self,
        name: &OsStr,
        write: impl FnOnce(&mut fs::File) -> io::Result<()>,
    ) -> io::Result<()> {
        validate_name(name)?;
        let temporary_name = format!(".codex-rehome-{}.tmp", Uuid::new_v4());
        let temporary_name = OsStr::new(&temporary_name);
        let mut temporary = create_file_at(self, temporary_name)
            .map_err(|error| io_stage("create pinned temporary file", error))?;
        let write_result = (|| {
            write(&mut temporary)
                .map_err(|error| io_stage("write pinned temporary file", error))?;
            temporary
                .sync_all()
                .map_err(|error| io_stage("flush pinned temporary file", error))
        })();
        drop(temporary);
        let result = write_result.and_then(|()| {
            self.verify_location()
                .map_err(|error| io_stage("verify pinned parent", error))?;
            replace_at(self, temporary_name, name)
                .map_err(|error| io_stage("replace pinned target", error))?;
            sync_directory_handle(&self.directory)
                .map_err(|error| io_stage("sync pinned parent", error))
        });
        if result.is_err() {
            let _ = remove_at(self, temporary_name);
        }
        result
    }

    fn verify_location(&self) -> io::Result<()> {
        let metadata = fs::symlink_metadata(&self.path)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() || is_reparse_point(&metadata) {
            return Err(io::Error::other("mutation parent changed identity"));
        }
        let current = open_directory_for_verification(&self.path)?;
        if directory_identity(&current)? != self.identity {
            return Err(io::Error::other("mutation parent changed identity"));
        }
        Ok(())
    }
}

fn io_stage(stage: &str, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("{stage}: {error}"))
}

#[cfg(unix)]
fn sync_directory_handle(directory: &fs::File) -> io::Result<()> {
    directory.sync_all()
}

#[cfg(not(unix))]
fn sync_directory_handle(_directory: &fs::File) -> io::Result<()> {
    Ok(())
}

fn validate_name(name: &OsStr) -> io::Result<()> {
    let path = Path::new(name);
    let mut components = path.components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "mutation target name is unsafe",
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn open_directory(path: &Path) -> io::Result<fs::File> {
    open_windows_directory(path, true)
}

#[cfg(windows)]
fn open_directory_for_verification(path: &Path) -> io::Result<fs::File> {
    open_windows_directory(path, true)
}

#[cfg(windows)]
fn open_windows_directory(path: &Path, share_delete: bool) -> io::Result<fs::File> {
    use std::os::windows::{ffi::OsStrExt, io::FromRawHandle};
    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        Storage::FileSystem::{
            CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        },
    };

    let path = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let mut sharing = FILE_SHARE_READ | FILE_SHARE_WRITE;
    if share_delete {
        sharing |= FILE_SHARE_DELETE;
    }
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            0,
            sharing,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { fs::File::from_raw_handle(handle) })
    }
}

#[cfg(windows)]
fn directory_identity(directory: &fs::File) -> io::Result<DirectoryIdentity> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY,
        FILE_ATTRIBUTE_REPARSE_POINT,
    };

    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    let result = unsafe { GetFileInformationByHandle(directory.as_raw_handle(), &mut information) };
    if result == 0 {
        return Err(io::Error::last_os_error());
    }
    if information.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
        || information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "mutation parent is not a regular directory",
        ));
    }
    Ok(DirectoryIdentity {
        first: u64::from(information.dwVolumeSerialNumber),
        second: (u64::from(information.nFileIndexHigh) << 32)
            | u64::from(information.nFileIndexLow),
    })
}

#[cfg(windows)]
fn create_file_at(parent: &PinnedParent, name: &OsStr) -> io::Result<fs::File> {
    parent.verify_location()?;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(parent.path.join(name))
}

#[cfg(windows)]
fn open_file_at(parent: &PinnedParent, name: &OsStr) -> io::Result<fs::File> {
    parent.verify_location()?;
    fs::File::open(parent.path.join(name))
}

#[cfg(windows)]
fn open_file_for_write_at(parent: &PinnedParent, name: &OsStr) -> io::Result<fs::File> {
    parent.verify_location()?;
    fs::OpenOptions::new()
        .write(true)
        .open(parent.path.join(name))
}

#[cfg(windows)]
fn replace_at(parent: &PinnedParent, source: &OsStr, destination: &OsStr) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    parent.verify_location()?;
    let source = parent
        .path
        .join(source)
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let destination = parent
        .path
        .join(destination)
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let moved = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn remove_at(parent: &PinnedParent, name: &OsStr) -> io::Result<()> {
    parent.verify_location()?;
    fs::remove_file(parent.path.join(name))
}

#[cfg(windows)]
fn set_permissions_at(
    parent: &PinnedParent,
    name: &OsStr,
    permissions: fs::Permissions,
) -> io::Result<()> {
    parent.verify_location()?;
    fs::set_permissions(parent.path.join(name), permissions)
}

#[cfg(windows)]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes()
        & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
        != 0
}

#[cfg(unix)]
fn open_directory(path: &Path) -> io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
}

#[cfg(unix)]
fn open_directory_for_verification(path: &Path) -> io::Result<fs::File> {
    open_directory(path)
}

#[cfg(unix)]
fn directory_identity(directory: &fs::File) -> io::Result<DirectoryIdentity> {
    use std::os::unix::fs::MetadataExt;
    let metadata = directory.metadata()?;
    Ok(DirectoryIdentity {
        first: metadata.dev(),
        second: metadata.ino(),
    })
}

#[cfg(unix)]
fn create_file_at(parent: &PinnedParent, name: &OsStr) -> io::Result<fs::File> {
    openat(
        parent,
        name,
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        0o600,
    )
}

#[cfg(unix)]
fn open_file_at(parent: &PinnedParent, name: &OsStr) -> io::Result<fs::File> {
    openat(parent, name, libc::O_RDONLY, 0)
}

#[cfg(unix)]
fn open_file_for_write_at(parent: &PinnedParent, name: &OsStr) -> io::Result<fs::File> {
    openat(parent, name, libc::O_WRONLY, 0)
}

#[cfg(unix)]
fn openat(
    parent: &PinnedParent,
    name: &OsStr,
    flags: i32,
    mode: libc::mode_t,
) -> io::Result<fs::File> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let name = unix_name(name)?;
    let descriptor = unsafe {
        libc::openat(
            parent.directory.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            mode,
        )
    };
    if descriptor < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { fs::File::from_raw_fd(descriptor) })
    }
}

#[cfg(unix)]
fn replace_at(parent: &PinnedParent, source: &OsStr, destination: &OsStr) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    let source = unix_name(source)?;
    let destination = unix_name(destination)?;
    let result = unsafe {
        libc::renameat(
            parent.directory.as_raw_fd(),
            source.as_ptr(),
            parent.directory.as_raw_fd(),
            destination.as_ptr(),
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(unix)]
fn remove_at(parent: &PinnedParent, name: &OsStr) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    let name = unix_name(name)?;
    let result = unsafe { libc::unlinkat(parent.directory.as_raw_fd(), name.as_ptr(), 0) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(unix)]
fn set_permissions_at(
    parent: &PinnedParent,
    name: &OsStr,
    permissions: fs::Permissions,
) -> io::Result<()> {
    open_file_at(parent, name)?.set_permissions(permissions)
}

#[cfg(unix)]
fn unix_name(name: &OsStr) -> io::Result<std::ffi::CString> {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::CString::new(name.as_bytes()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "mutation target name contains NUL",
        )
    })
}

#[cfg(unix)]
fn is_reparse_point(_metadata: &fs::Metadata) -> bool {
    false
}
