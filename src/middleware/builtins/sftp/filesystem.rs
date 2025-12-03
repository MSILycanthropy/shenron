use std::{
    fs, io,
    os::unix::fs::{MetadataExt, PermissionsExt},
};

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
    /// This function will return an error if file/link doesnt exist
    /// or getting the info fails.
    fn lstat(&self, path: &str) -> io::Result<FileAttr>;

    /// Open a file for reading
    ///
    /// # Errors
    ///
    /// This function will return an error if the file doesnt exist or
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

    /// Get the canonical path of a file
    ///
    /// # Errors
    ///
    /// This function will return an error if the file is unable to be canonicalized.
    fn realpath(&self, path: &str) -> io::Result<String>;
}

pub trait FileHandle: Send + Sync {
    fn read(&mut self, offset: u64, len: u32) -> std::io::Result<Vec<u8>>;
    fn write(&mut self, offset: u64, data: &[u8]) -> std::io::Result<u32>;
    fn stat(&self) -> std::io::Result<FileAttr>;
    fn set_stat(&mut self, attrs: FileAttr) -> std::io::Result<()>;
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

impl TryFrom<fs::Metadata> for FileAttr {
    type Error = io::Error;

    fn try_from(value: fs::Metadata) -> Result<Self, Self::Error> {
        let atime = u32::try_from(value.atime()).map_err(io::Error::other)?;
        let mtime = u32::try_from(value.mtime()).map_err(io::Error::other)?;

        Ok(Self {
            size: Some(value.len()),
            uid: Some(value.uid()),
            gid: Some(value.gid()),
            permissions: Some(value.permissions().mode()),
            atime: Some(atime),
            mtime: Some(mtime),
        })
    }
}
