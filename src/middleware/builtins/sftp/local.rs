use std::{
    io::{self, Read, Seek, SeekFrom, Write},
    path::Path,
    sync::Arc,
};

use cap_std::{
    ambient_authority,
    fs::{Dir, File, Metadata, MetadataExt, OpenOptions, Permissions, PermissionsExt},
};
use russh_sftp::protocol::OpenFlags;

use crate::middleware::builtins::sftp::filesystem::{DirEntry, FileAttr, FileHandle, Filesystem};

/// A [`Filesystem`] backed by a real directory on disk.
///
/// All operations are sandboxed to the root directory via [`cap_std`], which
/// resolves paths with `openat2`/`RESOLVE_BENEATH` semantics. Path traversal
/// (`../`) and symlinks escaping the root are rejected by the kernel, not by
/// string munging.
#[derive(Clone)]
pub struct LocalFilesystem {
    root: Arc<Dir>,
}

impl LocalFilesystem {
    /// Open `root` as the sandbox for all SFTP operations.
    ///
    /// # Panics
    ///
    /// Panics if `root` cannot be opened as a directory. Use
    /// [`LocalFilesystem::try_new`] to handle the error instead.
    #[must_use]
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self::try_new(root).expect("failed to open SFTP root directory")
    }

    /// Open `root` as the sandbox for all SFTP operations.
    ///
    /// # Errors
    ///
    /// Returns `Err` if `root` cannot be opened as a directory.
    pub fn try_new(root: impl AsRef<Path>) -> io::Result<Self> {
        let dir = Dir::open_ambient_dir(root, ambient_authority())?;

        Ok(Self {
            root: Arc::new(dir),
        })
    }
}

/// SFTP paths are absolute (`/foo/bar`); cap-std treats paths as relative to
/// the root, so strip the leading separator. The root itself maps to `.`.
fn rel(path: &str) -> &str {
    let trimmed = path.trim_start_matches('/');

    if trimmed.is_empty() { "." } else { trimmed }
}

fn meta_to_attr(meta: &Metadata) -> FileAttr {
    FileAttr {
        size: Some(meta.len()),
        uid: Some(meta.uid()),
        gid: Some(meta.gid()),
        permissions: Some(meta.mode()),
        atime: u32::try_from(meta.atime()).ok(),
        mtime: u32::try_from(meta.mtime()).ok(),
    }
}

impl Filesystem for LocalFilesystem {
    fn read_dir(&self, path: &str) -> io::Result<Vec<DirEntry>> {
        let mut entries = vec![];

        for entry in self.root.read_dir(rel(path))? {
            let entry = entry?;
            let meta = entry.metadata()?;

            entries.push(DirEntry {
                name: entry.file_name().to_string_lossy().to_string(),
                attrs: meta_to_attr(&meta),
            });
        }

        Ok(entries)
    }

    fn stat(&self, path: &str) -> io::Result<FileAttr> {
        Ok(meta_to_attr(&self.root.metadata(rel(path))?))
    }

    fn lstat(&self, path: &str) -> io::Result<FileAttr> {
        Ok(meta_to_attr(&self.root.symlink_metadata(rel(path))?))
    }

    fn open_read(&self, path: &str) -> io::Result<Box<dyn FileHandle>> {
        let file = self.root.open(rel(path))?;

        Ok(Box::new(LocalFile::new(file)))
    }

    fn open_write(&self, path: &str, flags: OpenFlags) -> io::Result<Box<dyn FileHandle>> {
        let mut opts = OpenOptions::new();

        opts.write(true)
            .read(flags.contains(OpenFlags::READ))
            .append(flags.contains(OpenFlags::APPEND))
            .truncate(flags.contains(OpenFlags::TRUNCATE));

        if flags.contains(OpenFlags::CREATE) {
            if flags.contains(OpenFlags::EXCLUDE) {
                opts.create_new(true);
            } else {
                opts.create(true);
            }
        }

        let file = self.root.open_with(rel(path), &opts)?;

        Ok(Box::new(LocalFile::new(file)))
    }

    fn mkdir(&self, path: &str, _attrs: FileAttr) -> io::Result<()> {
        self.root.create_dir(rel(path))
    }

    fn rmdir(&self, path: &str) -> io::Result<()> {
        self.root.remove_dir(rel(path))
    }

    fn remove(&self, path: &str) -> io::Result<()> {
        self.root.remove_file(rel(path))
    }

    fn rename(&self, from: &str, to: &str) -> io::Result<()> {
        self.root.rename(rel(from), &self.root, rel(to))
    }

    fn set_stat(&self, path: &str, attrs: FileAttr) -> io::Result<()> {
        if let Some(mode) = attrs.permissions {
            self.root
                .set_permissions(rel(path), Permissions::from_mode(mode))?;
        }

        if let Some(size) = attrs.size {
            self.root
                .open_with(rel(path), OpenOptions::new().write(true))?
                .set_len(size)?;
        }

        Ok(())
    }

    fn realpath(&self, path: &str) -> io::Result<String> {
        let canonical = self.root.canonicalize(rel(path))?;
        let virtual_path = canonical.to_string_lossy();

        if virtual_path.is_empty() {
            Ok("/".to_string())
        } else {
            Ok(format!("/{virtual_path}"))
        }
    }
}

struct LocalFile {
    file: File,
}

impl LocalFile {
    const fn new(file: File) -> Self {
        Self { file }
    }
}

impl FileHandle for LocalFile {
    fn read(&mut self, offset: u64, len: u32) -> io::Result<Vec<u8>> {
        self.file.seek(SeekFrom::Start(offset))?;

        let mut buffer = vec![0u8; len as usize];
        let len = self.file.read(&mut buffer)?;

        buffer.truncate(len);

        Ok(buffer)
    }

    fn write(&mut self, offset: u64, data: &[u8]) -> io::Result<u32> {
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.write_all(data)?;

        let length = u32::try_from(data.len()).map_err(io::Error::other)?;

        Ok(length)
    }

    fn stat(&self) -> io::Result<FileAttr> {
        Ok(meta_to_attr(&self.file.metadata()?))
    }

    fn set_stat(&mut self, attrs: FileAttr) -> io::Result<()> {
        if let Some(mode) = attrs.permissions {
            self.file.set_permissions(Permissions::from_mode(mode))?;
        }

        if let Some(size) = attrs.size {
            self.file.set_len(size)?;
        }

        Ok(())
    }

    fn close(self: Box<Self>) -> io::Result<()> {
        Ok(())
    }
}
