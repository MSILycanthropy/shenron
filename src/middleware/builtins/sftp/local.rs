use std::{
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom, Write},
    path::PathBuf,
};

use russh_sftp::protocol::OpenFlags;

use crate::middleware::builtins::sftp::filesystem::{DirEntry, FileAttr, FileHandle, Filesystem};

#[derive(Clone)]
pub struct LocalFilesystem {
    root: PathBuf,
}

impl LocalFilesystem {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn resolve(&self, path: &str) -> PathBuf {
        let path = path.trim_start_matches('/');

        self.root.join(path)
    }
}

impl Filesystem for LocalFilesystem {
    fn read_dir(&self, path: &str) -> io::Result<Vec<DirEntry>> {
        let full = self.resolve(path);

        let mut entries = vec![];

        for entry in fs::read_dir(full)? {
            let entry = entry?;
            let meta = entry.metadata()?;

            entries.push(DirEntry {
                name: entry.file_name().to_string_lossy().to_string(),
                attrs: meta.try_into()?,
            });
        }

        Ok(entries)
    }

    fn stat(&self, path: &str) -> io::Result<FileAttr> {
        let meta = fs::metadata(self.resolve(path))?;

        meta.try_into()
    }

    fn lstat(&self, path: &str) -> io::Result<FileAttr> {
        let meta = fs::symlink_metadata(self.resolve(path))?;

        meta.try_into()
    }

    fn open_read(&self, path: &str) -> io::Result<Box<dyn FileHandle>> {
        let file = File::open(self.resolve(path))?;

        Ok(Box::new(LocalFile::new(file)))
    }

    fn open_write(&self, path: &str, _flags: OpenFlags) -> io::Result<Box<dyn FileHandle>> {
        let file = File::open(self.resolve(path))?;

        Ok(Box::new(LocalFile::new(file)))
    }

    fn mkdir(&self, path: &str, _attrs: FileAttr) -> io::Result<()> {
        fs::create_dir(self.resolve(path))
    }

    fn rmdir(&self, path: &str) -> io::Result<()> {
        fs::remove_dir(self.resolve(path))
    }

    fn remove(&self, path: &str) -> io::Result<()> {
        fs::remove_file(self.resolve(path))
    }

    fn rename(&self, from: &str, to: &str) -> io::Result<()> {
        fs::rename(self.resolve(from), self.resolve(to))
    }

    fn realpath(&self, path: &str) -> io::Result<String> {
        let full = self.resolve(path).canonicalize()?;
        Ok(full.to_string_lossy().to_string())
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
        self.file.metadata()?.try_into()
    }

    fn set_stat(&mut self, _attrs: FileAttr) -> io::Result<()> {
        Ok(())
    }

    fn close(self: Box<Self>) -> std::io::Result<()> {
        Ok(())
    }
}
