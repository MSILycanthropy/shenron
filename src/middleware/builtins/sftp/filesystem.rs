use std::io;

use russh_sftp::protocol::{FileAttributes, OpenFlags};

/// Trait for filesystem operations.
///
/// Methods are async and run on the tokio runtime, so implementations must
/// not block: backends doing synchronous I/O offload to
/// [`tokio::task::spawn_blocking`] (see `LocalFilesystem`), while
/// network-backed stores can be natively async.
#[trait_variant::make(Send)]
pub trait Filesystem: Sync + Clone + 'static {
    /// Open-file handle returned by [`Filesystem::open_read`] /
    /// [`Filesystem::open_write`].
    type Handle: FileHandle;

    /// Read from a directory
    ///
    /// # Errors
    ///
    /// This function will return an error if the dir fails to be read
    async fn read_dir(&self, path: &str) -> io::Result<Vec<DirEntry>>;

    /// Return the information about a file
    ///
    /// # Errors
    ///
    /// This function will return an error if the file doesn't exist.
    async fn stat(&self, path: &str) -> io::Result<FileAttr>;

    /// Retrieve information about a file or symbolic link.
    ///
    /// # Errors
    ///
    /// This function will return an error if file/link doesn't exist
    /// or getting the info fails.
    async fn lstat(&self, path: &str) -> io::Result<FileAttr>;

    /// Open a file for reading
    ///
    /// # Errors
    ///
    /// This function will return an error if the file doesn't exist or
    /// other OS errors
    async fn open_read(&self, path: &str) -> io::Result<Self::Handle>;

    /// Open a file for writing. `attrs.permissions` applies when the file is
    /// created (masked with `0o7777`, like OpenSSH); ignored for existing files.
    ///
    /// # Errors
    ///
    /// This function will return an error if the file doesn't exist or
    /// there are other OS errors
    async fn open_write(
        &self,
        path: &str,
        flags: OpenFlags,
        attrs: FileAttr,
    ) -> io::Result<Self::Handle>;

    /// Make a directory
    ///
    /// # Errors
    ///
    /// This function will return an error if making the directory fails at the OS level.
    async fn mkdir(&self, path: &str, attrs: FileAttr) -> io::Result<()>;

    /// Remove a directory
    ///
    /// # Errors
    ///
    /// This function will return an error if removing the directory fails at the OS level.
    async fn rmdir(&self, path: &str) -> io::Result<()>;

    /// Remove a file
    ///
    /// # Errors
    ///
    /// This function will return an error if removing the file fails at the OS level.
    async fn remove(&self, path: &str) -> io::Result<()>;

    /// Rename a file
    ///
    /// # Errors
    ///
    /// This function will return an error if renaming the file fails at the OS level.
    async fn rename(&self, from: &str, to: &str) -> io::Result<()>;

    /// Apply attributes (e.g. permissions, size) to a path.
    ///
    /// Backends without attribute support should return
    /// [`io::ErrorKind::Unsupported`], which the SFTP handler reports as
    /// `SSH_FX_OP_UNSUPPORTED`.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend does not support setting attributes or
    /// the operation fails at the OS level.
    async fn set_stat(&self, path: &str, attrs: FileAttr) -> io::Result<()>;

    /// Get the canonical path of a file
    ///
    /// # Errors
    ///
    /// This function will return an error if the file is unable to be canonicalized.
    async fn realpath(&self, path: &str) -> io::Result<String>;
}

/// An open file, returned by [`Filesystem::open_read`] / [`Filesystem::open_write`].
///
/// Methods are async; the same no-blocking rule as [`Filesystem`] applies.
///
/// All methods return an error on the usual OS-level failures (bad offset,
/// permission denied, I/O error, etc.).
#[trait_variant::make(Send)]
pub trait FileHandle: 'static {
    /// Read up to `len` bytes starting at `offset`.
    ///
    /// # Errors
    ///
    /// Returns an error if the read fails at the OS level.
    async fn read(&mut self, offset: u64, len: u32) -> io::Result<Vec<u8>>;

    /// Write `data` starting at `offset`, returning the number of bytes written.
    ///
    /// # Errors
    ///
    /// Returns an error if the write fails at the OS level.
    async fn write(&mut self, offset: u64, data: Vec<u8>) -> io::Result<u32>;

    /// Return attributes for the open file.
    ///
    /// # Errors
    ///
    /// Returns an error if the attributes cannot be read.
    async fn stat(&self) -> io::Result<FileAttr>;

    /// Apply attributes (e.g. permissions, size) to the open file.
    ///
    /// # Errors
    ///
    /// Returns an error if the attributes cannot be applied.
    async fn set_stat(&mut self, attrs: FileAttr) -> io::Result<()>;

    /// Close the file, flushing any pending state.
    ///
    /// # Errors
    ///
    /// Returns an error if closing fails at the OS level.
    async fn close(self) -> io::Result<()>;
}

#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: String,
    pub attrs: FileAttr,
}

#[derive(Debug, Clone, Default)]
pub struct FileAttr {
    pub size: Option<u64>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub permissions: Option<u32>,
    pub atime: Option<u32>,
    pub mtime: Option<u32>,
}

impl From<FileAttr> for FileAttributes {
    fn from(val: FileAttr) -> Self {
        Self {
            size: val.size,
            uid: val.uid,
            gid: val.gid,
            permissions: val.permissions,
            atime: val.atime,
            mtime: val.mtime,
            ..Default::default()
        }
    }
}

impl From<FileAttributes> for FileAttr {
    fn from(val: FileAttributes) -> Self {
        Self {
            size: val.size,
            uid: val.uid,
            gid: val.gid,
            permissions: val.permissions,
            atime: val.atime,
            mtime: val.mtime,
        }
    }
}
