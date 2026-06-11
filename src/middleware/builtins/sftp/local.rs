use std::{io, path::Path, sync::Arc};

use cap_std::{
    ambient_authority,
    fs::{Dir, File, FileExt, Metadata, MetadataExt, OpenOptions, Permissions, PermissionsExt},
};
use russh_sftp::protocol::OpenFlags;

use crate::middleware::builtins::sftp::filesystem::{DirEntry, FileAttr, FileHandle, Filesystem};

/// A [`Filesystem`] backed by a real directory on disk.
///
/// All operations are sandboxed to the root directory via [`cap_std`], which
/// resolves paths with `openat2`/`RESOLVE_BENEATH` semantics. Path traversal
/// (`../`) and symlinks escaping the root are rejected by the kernel, not by
/// string munging.
///
/// Syscalls run on tokio's blocking thread pool
/// ([`tokio::task::spawn_blocking`]), so slow storage (NFS, network mounts)
/// stalls only the request, never the async runtime.
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

/// Run a blocking syscall on tokio's blocking pool. A `JoinError` means the
/// closure panicked; surface it as an I/O error rather than unwinding.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> io::Result<T> + Send + 'static,
) -> io::Result<T> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(io::Error::other)?
}

/// SFTP paths are absolute (`/foo/bar`); cap-std treats paths as relative to
/// the root, so strip the leading separator. The root itself maps to `.`.
fn rel(path: &str) -> String {
    let trimmed = path.trim_start_matches('/');

    if trimmed.is_empty() { "." } else { trimmed }.to_string()
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
    type Handle = LocalFile;

    async fn read_dir(&self, path: &str) -> io::Result<Vec<DirEntry>> {
        let root = Arc::clone(&self.root);
        let path = rel(path);

        blocking(move || {
            let mut entries = vec![];

            for entry in root.read_dir(path)? {
                let entry = entry?;
                let meta = entry.metadata()?;

                entries.push(DirEntry {
                    name: entry.file_name().to_string_lossy().to_string(),
                    attrs: meta_to_attr(&meta),
                });
            }

            Ok(entries)
        })
        .await
    }

    async fn stat(&self, path: &str) -> io::Result<FileAttr> {
        let root = Arc::clone(&self.root);
        let path = rel(path);

        blocking(move || Ok(meta_to_attr(&root.metadata(path)?))).await
    }

    async fn lstat(&self, path: &str) -> io::Result<FileAttr> {
        let root = Arc::clone(&self.root);
        let path = rel(path);

        blocking(move || Ok(meta_to_attr(&root.symlink_metadata(path)?))).await
    }

    async fn open_read(&self, path: &str) -> io::Result<LocalFile> {
        let root = Arc::clone(&self.root);
        let path = rel(path);

        blocking(move || Ok(LocalFile::new(root.open(path)?))).await
    }

    async fn open_write(
        &self,
        path: &str,
        flags: OpenFlags,
        attrs: FileAttr,
    ) -> io::Result<LocalFile> {
        let root = Arc::clone(&self.root);
        let path = rel(path);

        blocking(move || {
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

            // The mode only takes effect when the open creates the file, so the
            // client's upload permissions land at syscall time — no chmod window.
            #[cfg(unix)]
            if let Some(mode) = attrs.permissions {
                cap_std::fs::OpenOptionsExt::mode(&mut opts, mode & 0o7777);
            }

            Ok(LocalFile::new(root.open_with(path, &opts)?))
        })
        .await
    }

    async fn mkdir(&self, path: &str, attrs: FileAttr) -> io::Result<()> {
        let root = Arc::clone(&self.root);
        let path = rel(path);

        blocking(move || {
            #[cfg(unix)]
            if let Some(mode) = attrs.permissions {
                let mut builder = cap_std::fs::DirBuilder::new();
                cap_std::fs::DirBuilderExt::mode(&mut builder, mode & 0o7777);

                return root.create_dir_with(path, &builder);
            }

            root.create_dir(path)
        })
        .await
    }

    async fn rmdir(&self, path: &str) -> io::Result<()> {
        let root = Arc::clone(&self.root);
        let path = rel(path);

        blocking(move || root.remove_dir(path)).await
    }

    async fn remove(&self, path: &str) -> io::Result<()> {
        let root = Arc::clone(&self.root);
        let path = rel(path);

        blocking(move || root.remove_file(path)).await
    }

    async fn rename(&self, from: &str, to: &str) -> io::Result<()> {
        let root = Arc::clone(&self.root);
        let from = rel(from);
        let to = rel(to);

        blocking(move || root.rename(from, &root, to)).await
    }

    async fn set_stat(&self, path: &str, attrs: FileAttr) -> io::Result<()> {
        let root = Arc::clone(&self.root);
        let path = rel(path);

        blocking(move || {
            if let Some(mode) = attrs.permissions {
                root.set_permissions(&path, Permissions::from_mode(mode))?;
            }

            if let Some(size) = attrs.size {
                root.open_with(&path, OpenOptions::new().write(true))?
                    .set_len(size)?;
            }

            Ok(())
        })
        .await
    }

    async fn realpath(&self, path: &str) -> io::Result<String> {
        let root = Arc::clone(&self.root);
        let path = rel(path);

        blocking(move || {
            let canonical = root.canonicalize(path)?;
            let virtual_path = canonical.to_string_lossy();

            if virtual_path.is_empty() {
                Ok("/".to_string())
            } else {
                Ok(format!("/{virtual_path}"))
            }
        })
        .await
    }
}

/// Positional I/O (`read_at`/`write_at`) needs only `&File`, so the handle is
/// shared with the blocking pool via `Arc` instead of moved back and forth.
pub struct LocalFile {
    file: Arc<File>,
}

impl LocalFile {
    fn new(file: File) -> Self {
        Self {
            file: Arc::new(file),
        }
    }
}

impl FileHandle for LocalFile {
    async fn read(&mut self, offset: u64, len: u32) -> io::Result<Vec<u8>> {
        let file = Arc::clone(&self.file);

        blocking(move || {
            let mut buffer = vec![0u8; len as usize];
            let len = file.read_at(&mut buffer, offset)?;

            buffer.truncate(len);

            Ok(buffer)
        })
        .await
    }

    async fn write(&mut self, offset: u64, data: Vec<u8>) -> io::Result<u32> {
        let file = Arc::clone(&self.file);

        blocking(move || {
            file.write_all_at(&data, offset)?;

            u32::try_from(data.len()).map_err(io::Error::other)
        })
        .await
    }

    async fn stat(&self) -> io::Result<FileAttr> {
        let file = Arc::clone(&self.file);

        blocking(move || Ok(meta_to_attr(&file.metadata()?))).await
    }

    async fn set_stat(&mut self, attrs: FileAttr) -> io::Result<()> {
        let file = Arc::clone(&self.file);

        blocking(move || {
            if let Some(mode) = attrs.permissions {
                file.set_permissions(Permissions::from_mode(mode))?;
            }

            if let Some(size) = attrs.size {
                file.set_len(size)?;
            }

            Ok(())
        })
        .await
    }

    async fn close(self) -> io::Result<()> {
        Ok(())
    }
}
