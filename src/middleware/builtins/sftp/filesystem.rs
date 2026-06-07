use std::io;

use russh_sftp::protocol::{FileAttributes, OpenFlags};

/// Trait for filesystem operations
pub trait Filesystem: Send + Sync + Clone + 'static {
    /// Read from a directory
    ///
    /// # Errors
    ///
    /// This function will return an error if the dir fails to be read
    fn read_dir(&self, path: &str) -> io::Result<Vec<DirEntry>>;

    /// Return the information about a file
    ///
    /// # Errors
    ///
    /// This function will return an error if the file doesn't exist.
    fn stat(&self, path: &str) -> io::Result<FileAttr>;

    /// Retrieve information about a file or symbolic link.
    ///
    /// # Errors
    ///
    /// This function will return an error if file/link doesn't exist
    /// or getting the info fails.
    fn lstat(&self, path: &str) -> io::Result<FileAttr>;

    /// Open a file for reading
    ///
    /// # Errors
    ///
    /// This function will return an error if the file doesn't exist or
    /// other OS errors
    fn open_read(&self, path: &str) -> io::Result<Box<dyn FileHandle>>;

    /// Open a file for writing
    ///
    /// # Errors
    ///
    /// This function will return an error if the file doesn't exist or
    /// there are other OS errors
    fn open_write(&self, path: &str, flags: OpenFlags) -> io::Result<Box<dyn FileHandle>>;

    /// Make a directory
    ///
    /// # Errors
    ///
    /// This function will return an error if making the directory fails at the OS level.
    fn mkdir(&self, path: &str, attrs: FileAttr) -> io::Result<()>;

    /// Remove a directory
    ///
    /// # Errors
    ///
    /// This function will return an error if removing the directory fails at the OS level.
    fn rmdir(&self, path: &str) -> io::Result<()>;

    /// Remove a file
    ///
    /// # Errors
    ///
    /// This function will return an error if removing the file fails at the OS level.
    fn remove(&self, path: &str) -> io::Result<()>;

    /// Rename a file
    ///
    /// # Errors
    ///
    /// This function will return an error if renaming the file fails at the OS level.
    fn rename(&self, from: &str, to: &str) -> io::Result<()>;

    /// Apply attributes (e.g. permissions, size) to a path.
    ///
    /// The default implementation reports the operation as unsupported.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend does not support setting attributes or
    /// the operation fails at the OS level.
    fn set_stat(&self, _path: &str, _attrs: FileAttr) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "set_stat is not supported by this filesystem",
        ))
    }

    /// Get the canonical path of a file
    ///
    /// # Errors
    ///
    /// This function will return an error if the file is unable to be canonicalized.
    fn realpath(&self, path: &str) -> io::Result<String>;
}

/// An open file, returned by [`Filesystem::open_read`] / [`Filesystem::open_write`].
///
/// All methods return an error on the usual OS-level failures (bad offset,
/// permission denied, I/O error, etc.).
pub trait FileHandle: Send + Sync {
    /// Read up to `len` bytes starting at `offset`.
    ///
    /// # Errors
    ///
    /// Returns an error if the read fails at the OS level.
    fn read(&mut self, offset: u64, len: u32) -> std::io::Result<Vec<u8>>;

    /// Write `data` starting at `offset`, returning the number of bytes written.
    ///
    /// # Errors
    ///
    /// Returns an error if the write fails at the OS level.
    fn write(&mut self, offset: u64, data: &[u8]) -> std::io::Result<u32>;

    /// Return attributes for the open file.
    ///
    /// # Errors
    ///
    /// Returns an error if the attributes cannot be read.
    fn stat(&self) -> std::io::Result<FileAttr>;

    /// Apply attributes (e.g. permissions, size) to the open file.
    ///
    /// # Errors
    ///
    /// Returns an error if the attributes cannot be applied.
    fn set_stat(&mut self, attrs: FileAttr) -> std::io::Result<()>;

    /// Close the file, flushing any pending state.
    ///
    /// # Errors
    ///
    /// Returns an error if closing fails at the OS level.
    fn close(self: Box<Self>) -> std::io::Result<()>;
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
