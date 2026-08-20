#[cfg(test)]
mod tests {
    use super::PinnedParent;
    use std::{ffi::OsStr, fs};

    #[cfg(unix)]
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

    #[cfg(unix)]
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

    #[test]
    fn pinned_rename_never_replaces_a_concurrent_file() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("parent");
        fs::create_dir(&parent).unwrap();
        fs::write(parent.join("target"), b"applied").unwrap();
        let pinned = PinnedParent::open(&parent).unwrap();

        pinned
            .rename_child_if_absent(OsStr::new("target"), OsStr::new("quarantine"))
            .unwrap();
        fs::write(parent.join("target"), b"concurrent replacement").unwrap();
        let error = pinned
            .rename_child_if_absent(OsStr::new("quarantine"), OsStr::new("target"))
            .unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(
            fs::read(parent.join("target")).unwrap(),
            b"concurrent replacement"
        );
        assert_eq!(fs::read(parent.join("quarantine")).unwrap(), b"applied");
    }

    #[test]
    fn pinned_rename_handles_directories_without_replacing_a_concurrent_child() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("parent");
        fs::create_dir(&parent).unwrap();
        fs::create_dir(parent.join("target")).unwrap();
        fs::write(parent.join("target").join("unknown"), b"keep me").unwrap();
        let pinned = PinnedParent::open(&parent).unwrap();

        pinned
            .rename_child_if_absent(OsStr::new("target"), OsStr::new("quarantine"))
            .unwrap();
        fs::write(parent.join("target"), b"concurrent replacement").unwrap();
        let error = pinned
            .rename_child_if_absent(OsStr::new("quarantine"), OsStr::new("target"))
            .unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(
            fs::read(parent.join("target")).unwrap(),
            b"concurrent replacement"
        );
        assert_eq!(
            fs::read(parent.join("quarantine").join("unknown")).unwrap(),
            b"keep me"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_pin_blocks_parent_rename_and_swap_during_path_mutations() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("parent");
        let parked = root.path().join("parked");
        let outside = root.path().join("outside");
        let source = root.path().join("source");
        fs::create_dir(&parent).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(parent.join("replace"), b"original").unwrap();
        fs::write(parent.join("remove"), b"original").unwrap();
        fs::write(outside.join("replace"), b"outside").unwrap();
        fs::write(outside.join("remove"), b"outside").unwrap();
        fs::write(&source, b"replacement").unwrap();
        let pinned = PinnedParent::open(&parent).unwrap();

        assert!(fs::rename(&parent, &parked).is_err());
        assert!(fs::rename(&outside, &parent).is_err());
        pinned.replace_file(&source, OsStr::new("replace")).unwrap();
        pinned.remove_file(OsStr::new("remove")).unwrap();

        assert_eq!(fs::read(parent.join("replace")).unwrap(), b"replacement");
        assert!(!parent.join("remove").exists());
        assert_eq!(fs::read(outside.join("replace")).unwrap(), b"outside");
        assert_eq!(fs::read(outside.join("remove")).unwrap(), b"outside");
    }

    #[cfg(windows)]
    #[test]
    fn windows_pinned_replace_supports_paths_longer_than_max_path() {
        let root = tempfile::tempdir().unwrap();
        let long_component = "a".repeat(170);
        let parent = root.path().join(long_component);
        fs::create_dir(&parent).unwrap();
        let target_name = format!("{}-target.txt", "b".repeat(80));
        let target = parent.join(&target_name);
        assert!(target.as_os_str().to_string_lossy().len() > 260);
        let pinned = PinnedParent::open(&parent).unwrap();

        pinned
            .replace_bytes(OsStr::new(&target_name), b"long path works")
            .unwrap();

        assert_eq!(
            pinned
                .open_file(OsStr::new(&target_name))
                .unwrap()
                .metadata()
                .unwrap()
                .len(),
            15
        );
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

    pub(crate) fn install_file_if_absent(&self, source: &Path, name: &OsStr) -> io::Result<()> {
        validate_name(name)?;
        let temporary_name = format!(".codex-rehome-{}.tmp", Uuid::new_v4());
        let temporary_name = OsStr::new(&temporary_name);
        let mut source = fs::File::open(source)?;
        let mut temporary = create_file_at(self, temporary_name)
            .map_err(|error| io_stage("create pinned temporary file", error))?;
        let write_result = (|| {
            io::copy(&mut source, &mut temporary)
                .map(|_| ())
                .map_err(|error| io_stage("write pinned temporary file", error))?;
            temporary
                .sync_all()
                .map_err(|error| io_stage("flush pinned temporary file", error))
        })();
        drop(temporary);
        let result = write_result.and_then(|()| {
            self.rename_child_if_absent(temporary_name, name)
                .map_err(|error| io_stage("install pinned target", error))
        });
        if result.is_err() {
            let _ = remove_at(self, temporary_name);
        }
        result
    }

    pub(crate) fn rename_child_if_absent(
        &self,
        source: &OsStr,
        destination: &OsStr,
    ) -> io::Result<()> {
        validate_name(source)?;
        validate_name(destination)?;
        self.verify_location()?;
        rename_noreplace_at(self, source, destination)?;
        self.verify_location()?;
        sync_directory_handle(&self.directory)
    }

    pub(crate) fn remove_file(&self, name: &OsStr) -> io::Result<()> {
        validate_name(name)?;
        self.verify_location()?;
        remove_at(self, name)?;
        self.verify_location()?;
        sync_directory_handle(&self.directory)
    }

    pub(crate) fn open_file(&self, name: &OsStr) -> io::Result<fs::File> {
        validate_name(name)?;
        self.verify_location()?;
        open_file_at(self, name)
    }

    pub(crate) fn child_exists(&self, name: &OsStr) -> io::Result<bool> {
        validate_name(name)?;
        self.verify_location()?;
        child_exists_at(self, name)
    }

    pub(crate) fn create_new_file(&self, name: &OsStr) -> io::Result<fs::File> {
        validate_name(name)?;
        self.verify_location()?;
        let file = create_file_at(self, name)?;
        self.verify_location()?;
        Ok(file)
    }

    pub(crate) fn open_file_for_write(&self, name: &OsStr) -> io::Result<fs::File> {
        validate_name(name)?;
        self.verify_location()?;
        open_file_for_write_at(self, name)
    }

    pub(crate) fn sync(&self) -> io::Result<()> {
        sync_directory_handle(&self.directory)
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
            self.verify_location()
                .map_err(|error| io_stage("verify pinned parent after replace", error))?;
            sync_directory_handle(&self.directory)
                .map_err(|error| io_stage("sync pinned parent", error))
        });
        if result.is_err() {
            let _ = remove_at(self, temporary_name);
        }
        result
    }

    fn verify_location(&self) -> io::Result<()> {
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
    open_windows_directory(path, false)
}

#[cfg(windows)]
fn open_directory_for_verification(path: &Path) -> io::Result<fs::File> {
    open_windows_directory(path, true)
}

#[cfg(windows)]
fn open_windows_directory(path: &Path, share_delete: bool) -> io::Result<fs::File> {
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::{
        Foundation::{GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE},
        Storage::FileSystem::{
            CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        },
    };

    let path = windows_api_path(path);
    let mut sharing = FILE_SHARE_READ | FILE_SHARE_WRITE;
    if share_delete {
        sharing |= FILE_SHARE_DELETE;
    }
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
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
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::{
        Foundation::{GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE},
        Storage::FileSystem::{
            CreateFileW, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        },
    };

    parent.verify_location()?;
    let path = windows_api_path(&parent.path.join(name));
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT,
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
fn open_file_at(parent: &PinnedParent, name: &OsStr) -> io::Result<fs::File> {
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::{
        Foundation::{GENERIC_READ, INVALID_HANDLE_VALUE},
        Storage::FileSystem::{
            CreateFileW, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ,
            FILE_SHARE_WRITE, OPEN_EXISTING,
        },
    };

    parent.verify_location()?;
    let path = windows_api_path(&parent.path.join(name));
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT,
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
fn child_exists_at(parent: &PinnedParent, name: &OsStr) -> io::Result<bool> {
    use windows_sys::Win32::{
        Foundation::{GetLastError, ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND},
        Storage::FileSystem::{GetFileAttributesW, INVALID_FILE_ATTRIBUTES},
    };

    parent.verify_location()?;
    let path = windows_api_path(&parent.path.join(name));
    let attributes = unsafe { GetFileAttributesW(path.as_ptr()) };
    if attributes != INVALID_FILE_ATTRIBUTES {
        return Ok(true);
    }
    let error = unsafe { GetLastError() };
    if matches!(error, ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND) {
        Ok(false)
    } else {
        Err(io::Error::from_raw_os_error(error as i32))
    }
}

#[cfg(windows)]
fn open_file_for_write_at(parent: &PinnedParent, name: &OsStr) -> io::Result<fs::File> {
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::{
        Foundation::{GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE},
        Storage::FileSystem::{
            CreateFileW, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ,
            FILE_SHARE_WRITE, FILE_WRITE_ATTRIBUTES, OPEN_EXISTING,
        },
    };

    parent.verify_location()?;
    let path = windows_api_path(&parent.path.join(name));
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            GENERIC_READ | GENERIC_WRITE | FILE_WRITE_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT,
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
fn replace_at(parent: &PinnedParent, source: &OsStr, destination: &OsStr) -> io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    parent.verify_location()?;
    let source = windows_api_path(&parent.path.join(source));
    let destination = windows_api_path(&parent.path.join(destination));
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
fn rename_noreplace_at(
    parent: &PinnedParent,
    source: &OsStr,
    destination: &OsStr,
) -> io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};

    parent.verify_location()?;
    let source = windows_api_path(&parent.path.join(source));
    let destination = windows_api_path(&parent.path.join(destination));
    let moved = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_WRITE_THROUGH,
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
    use windows_sys::Win32::Storage::FileSystem::DeleteFileW;

    parent.verify_location()?;
    let path = windows_api_path(&parent.path.join(name));
    if unsafe { DeleteFileW(path.as_ptr()) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn windows_api_path(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;

    let raw = path.as_os_str().encode_wide().collect::<Vec<_>>();
    const BACKSLASH: u16 = b'\\' as u16;
    const QUESTION: u16 = b'?' as u16;
    if raw.starts_with(&[BACKSLASH, BACKSLASH, QUESTION, BACKSLASH]) {
        return raw.into_iter().chain(Some(0)).collect();
    }
    let mut extended = Vec::with_capacity(raw.len() + 9);
    if raw.starts_with(&[BACKSLASH, BACKSLASH]) {
        extended.extend(r"\\?\UNC\".encode_utf16());
        extended.extend_from_slice(&raw[2..]);
    } else {
        extended.extend(r"\\?\".encode_utf16());
        extended.extend_from_slice(&raw);
    }
    extended.push(0);
    extended
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
fn child_exists_at(parent: &PinnedParent, name: &OsStr) -> io::Result<bool> {
    use std::os::fd::AsRawFd;
    let name = unix_name(name)?;
    let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
    let result = unsafe {
        libc::fstatat(
            parent.directory.as_raw_fd(),
            name.as_ptr(),
            metadata.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result == 0 {
        Ok(true)
    } else {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::NotFound {
            Ok(false)
        } else {
            Err(error)
        }
    }
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
            mode as libc::c_uint,
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

#[cfg(any(target_os = "linux", target_os = "android"))]
fn rename_noreplace_at(
    parent: &PinnedParent,
    source: &OsStr,
    destination: &OsStr,
) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    let source = unix_name(source)?;
    let destination = unix_name(destination)?;
    let result = unsafe {
        libc::renameat2(
            parent.directory.as_raw_fd(),
            source.as_ptr(),
            parent.directory.as_raw_fd(),
            destination.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn rename_noreplace_at(
    parent: &PinnedParent,
    source: &OsStr,
    destination: &OsStr,
) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    let source = unix_name(source)?;
    let destination = unix_name(destination)?;
    let result = unsafe {
        libc::renameatx_np(
            parent.directory.as_raw_fd(),
            source.as_ptr(),
            parent.directory.as_raw_fd(),
            destination.as_ptr(),
            libc::RENAME_EXCL,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(all(
    unix,
    not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))
))]
fn rename_noreplace_at(
    _parent: &PinnedParent,
    _source: &OsStr,
    _destination: &OsStr,
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "atomic no-replace rename is not supported on this platform",
    ))
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
fn unix_name(name: &OsStr) -> io::Result<std::ffi::CString> {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::CString::new(name.as_bytes()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "mutation target name contains NUL",
        )
    })
}
