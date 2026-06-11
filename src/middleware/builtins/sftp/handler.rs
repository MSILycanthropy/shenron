use std::{
    collections::HashMap,
    io,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use russh_sftp::protocol::{
    Attrs, Data, FileAttributes, Handle, Name, OpenFlags, Status, StatusCode, Version,
};

use crate::middleware::builtins::sftp::filesystem::{DirEntry, FileAttr, FileHandle, Filesystem};

/// `len` in `SSH_FXP_READ` is client-controlled; clamp it so a hostile
/// `len = u32::MAX` can't force a 4 GiB allocation. Short reads are legal —
/// clients re-request the remainder. Matches russh-sftp's packet cap.
const MAX_READ_LEN: u32 = 256 * 1024;

/// Entries per `SSH_FXP_READDIR` response; keeps Name packets well under
/// client packet caps for large directories.
const READDIR_PAGE: usize = 128;

/// Internal handler that implements `russh_sftp::server::Handler`
pub struct SftpHandler<F: Filesystem> {
    fs: F,
    handles: HashMap<String, HandleType<F::Handle>>,
    next_handle: AtomicU64,

    version: Option<u32>,
}

enum HandleType<H> {
    File(H),
    Dir {
        entries: Vec<DirEntry>,
        offset: usize,
    },
}

impl<F: Filesystem> SftpHandler<F> {
    pub fn new(fs: F) -> Self {
        Self {
            fs,
            handles: HashMap::new(),
            next_handle: AtomicU64::new(0),

            version: None,
        }
    }

    fn next_handle(&self) -> String {
        let id = self.next_handle.fetch_add(1, Ordering::SeqCst);

        format!("{id:016x}")
    }
}

impl<F: Filesystem> russh_sftp::server::Handler for SftpHandler<F> {
    type Error = StatusCode;

    fn unimplemented(&self) -> Self::Error {
        StatusCode::OpUnsupported
    }

    async fn init(
        &mut self,
        version: u32,
        _extensions: HashMap<String, String>,
    ) -> Result<Version, Self::Error> {
        if self.version.is_some() {
            tracing::error!("duplicate SSH_FXP_VERSION packet");
            return Err(StatusCode::ConnectionLost);
        }

        self.version = Some(version);
        Ok(Version::new())
    }

    async fn close(&mut self, id: u32, handle: String) -> Result<Status, Self::Error> {
        match self.handles.remove(&handle) {
            Some(HandleType::File(f)) => f.close().await.map_err(|e| status_code(&e))?,
            Some(HandleType::Dir { .. }) => {}
            None => return Err(StatusCode::Failure),
        }

        status_ok(id)
    }

    async fn open(
        &mut self,
        id: u32,
        filename: String,
        pflags: OpenFlags,
        attrs: FileAttributes,
    ) -> Result<Handle, Self::Error> {
        let handle = self.next_handle();

        let file = if pflags.contains(OpenFlags::WRITE) || pflags.contains(OpenFlags::CREATE) {
            self.fs.open_write(&filename, pflags, attrs.into()).await
        } else {
            self.fs.open_read(&filename).await
        }
        .map_err(|e| status_code(&e))?;

        self.handles.insert(handle.clone(), HandleType::File(file));

        Ok(Handle { id, handle })
    }

    async fn read(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        len: u32,
    ) -> Result<Data, Self::Error> {
        let Some(HandleType::File(f)) = self.handles.get_mut(&handle) else {
            return Err(StatusCode::Failure);
        };

        let data = f
            .read(offset, len.min(MAX_READ_LEN))
            .await
            .map_err(|e| status_code(&e))?;

        if data.is_empty() {
            return Err(StatusCode::Eof);
        }

        Ok(Data { id, data })
    }

    async fn write(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        data: Vec<u8>,
    ) -> Result<Status, Self::Error> {
        let Some(HandleType::File(f)) = self.handles.get_mut(&handle) else {
            return Err(StatusCode::Failure);
        };

        f.write(offset, data).await.map_err(|e| status_code(&e))?;

        status_ok(id)
    }

    async fn opendir(&mut self, id: u32, path: String) -> Result<Handle, Self::Error> {
        let entries = self.fs.read_dir(&path).await.map_err(|e| status_code(&e))?;
        let handle = self.next_handle();

        self.handles
            .insert(handle.clone(), HandleType::Dir { entries, offset: 0 });

        Ok(Handle { id, handle })
    }

    async fn readdir(&mut self, id: u32, handle: String) -> Result<Name, Self::Error> {
        let Some(HandleType::Dir { entries, offset }) = self.handles.get_mut(&handle) else {
            return Err(StatusCode::Failure);
        };

        if *offset >= entries.len() {
            return Err(StatusCode::Eof);
        }

        let now = unix_now();
        let end = (*offset + READDIR_PAGE).min(entries.len());
        let files: Vec<_> = entries[*offset..end]
            .iter()
            .map(|e| russh_sftp::protocol::File {
                filename: e.name.clone(),
                longname: longname(&e.name, &e.attrs, now),
                attrs: e.attrs.clone().into(),
            })
            .collect();

        *offset = end;

        Ok(Name { id, files })
    }

    async fn realpath(&mut self, id: u32, path: String) -> Result<Name, Self::Error> {
        let real = self.fs.realpath(&path).await.map_err(|e| status_code(&e))?;

        Ok(Name {
            id,
            files: vec![russh_sftp::protocol::File {
                filename: real,
                longname: "Ok".to_string(),
                attrs: FileAttributes::default(),
            }],
        })
    }

    async fn stat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        let attrs = self.fs.stat(&path).await.map_err(|e| status_code(&e))?;

        Ok(Attrs {
            id,
            attrs: attrs.into(),
        })
    }

    async fn lstat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        let attrs = self.fs.lstat(&path).await.map_err(|e| status_code(&e))?;

        Ok(Attrs {
            id,
            attrs: attrs.into(),
        })
    }

    async fn fstat(&mut self, id: u32, handle: String) -> Result<Attrs, Self::Error> {
        let Some(HandleType::File(f)) = self.handles.get_mut(&handle) else {
            return Err(StatusCode::Failure);
        };

        let attrs = f.stat().await.map_err(|e| status_code(&e))?;

        Ok(Attrs {
            id,
            attrs: attrs.into(),
        })
    }

    async fn remove(&mut self, id: u32, filename: String) -> Result<Status, Self::Error> {
        self.fs.remove(&filename).await.map_err(|e| status_code(&e))?;

        status_ok(id)
    }

    async fn mkdir(
        &mut self,
        id: u32,
        path: String,
        attrs: FileAttributes,
    ) -> Result<Status, Self::Error> {
        self.fs
            .mkdir(&path, attrs.into())
            .await
            .map_err(|e| status_code(&e))?;

        status_ok(id)
    }

    async fn rmdir(&mut self, id: u32, path: String) -> Result<Status, Self::Error> {
        self.fs.rmdir(&path).await.map_err(|e| status_code(&e))?;

        status_ok(id)
    }

    async fn rename(
        &mut self,
        id: u32,
        oldpath: String,
        newpath: String,
    ) -> Result<Status, Self::Error> {
        self.fs
            .rename(&oldpath, &newpath)
            .await
            .map_err(|e| status_code(&e))?;

        status_ok(id)
    }

    async fn setstat(
        &mut self,
        id: u32,
        path: String,
        attrs: FileAttributes,
    ) -> Result<Status, Self::Error> {
        self.fs
            .set_stat(&path, attrs.into())
            .await
            .map_err(|e| status_code(&e))?;

        status_ok(id)
    }

    async fn fsetstat(
        &mut self,
        id: u32,
        handle: String,
        attrs: FileAttributes,
    ) -> Result<Status, Self::Error> {
        let Some(HandleType::File(f)) = self.handles.get_mut(&handle) else {
            return Err(StatusCode::Failure);
        };

        f.set_stat(attrs.into())
            .await
            .map_err(|e| status_code(&e))?;

        status_ok(id)
    }
}

#[allow(clippy::unnecessary_wraps)]
fn status_ok(id: u32) -> Result<Status, StatusCode> {
    Ok(Status {
        id,
        status_code: StatusCode::Ok,
        error_message: "Ok".to_string(),
        language_tag: "en-US".to_string(),
    })
}

/// SFTP v3 has no "already exists" code, so `AlreadyExists` (e.g. EEXIST
/// under `CREATE|EXCLUDE`) falls through to the generic `Failure`.
fn status_code(err: &io::Error) -> StatusCode {
    match err.kind() {
        io::ErrorKind::NotFound => StatusCode::NoSuchFile,
        io::ErrorKind::PermissionDenied => StatusCode::PermissionDenied,
        io::ErrorKind::Unsupported => StatusCode::OpUnsupported,
        _ => StatusCode::Failure,
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// `ls -l`-style line clients display verbatim for `SSH_FXP_READDIR`
/// entries. Mirrors OpenSSH's sftp-server, except the link count isn't
/// tracked by [`FileAttr`] and is always reported as 1.
fn longname(name: &str, attrs: &FileAttr, now: i64) -> String {
    format!(
        "{} {:>3} {:<8} {:<8} {:>8} {} {}",
        mode_string(attrs.permissions.unwrap_or(0)),
        1,
        attrs.uid.unwrap_or(0),
        attrs.gid.unwrap_or(0),
        attrs.size.unwrap_or(0),
        mtime_string(attrs.mtime.unwrap_or(0), now),
        name,
    )
}

fn mode_string(mode: u32) -> String {
    let kind = match mode & 0o170_000 {
        0o140_000 => 's',
        0o120_000 => 'l',
        0o060_000 => 'b',
        0o040_000 => 'd',
        0o020_000 => 'c',
        0o010_000 => 'p',
        _ => '-',
    };

    let mut out = String::with_capacity(10);
    out.push(kind);

    for shift in [6, 3, 0] {
        let bits = mode >> shift;
        out.push(if bits & 0o4 == 0 { '-' } else { 'r' });
        out.push(if bits & 0o2 == 0 { '-' } else { 'w' });
        out.push(if bits & 0o1 == 0 { '-' } else { 'x' });
    }

    out
}

/// Like `ls -l`: time of day for recent files, year for older ones.
fn mtime_string(mtime: u32, now: i64) -> String {
    const SIX_MONTHS: i64 = 182 * 24 * 60 * 60;

    let mtime = i64::from(mtime);
    let format = if mtime > now - SIX_MONTHS {
        "%b %e %H:%M"
    } else {
        "%b %e  %Y"
    };

    chrono::DateTime::from_timestamp(mtime, 0)
        .map_or_else(String::new, |dt| dt.format(format).to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use russh_sftp::server::Handler;
    use tempfile::TempDir;

    use super::*;
    use crate::middleware::builtins::sftp::LocalFilesystem;

    fn handler(tmp: &TempDir) -> SftpHandler<LocalFilesystem> {
        SftpHandler::new(LocalFilesystem::new(tmp.path()))
    }

    #[tokio::test]
    async fn readdir_pages_large_directories() {
        let tmp = TempDir::new().expect("tempdir");
        for i in 0..300 {
            fs::write(tmp.path().join(format!("file{i:03}")), b"x").expect("write");
        }

        let mut h = handler(&tmp);
        let dir = h.opendir(0, "/".into()).await.expect("opendir").handle;

        let mut pages = vec![];
        loop {
            match h.readdir(1, dir.clone()).await {
                Ok(name) => pages.push(name.files.len()),
                Err(StatusCode::Eof) => break,
                Err(other) => panic!("unexpected status: {other:?}"),
            }
        }

        assert_eq!(pages, vec![128, 128, 44]);
    }

    #[tokio::test]
    async fn readdir_empty_directory_is_eof() {
        let tmp = TempDir::new().expect("tempdir");
        let mut h = handler(&tmp);

        let dir = h.opendir(0, "/".into()).await.expect("opendir").handle;

        assert!(matches!(h.readdir(1, dir).await, Err(StatusCode::Eof)));
    }

    #[tokio::test]
    async fn fstat_returns_open_file_attrs() {
        let tmp = TempDir::new().expect("tempdir");
        fs::write(tmp.path().join("data"), b"hello").expect("write");

        let mut h = handler(&tmp);
        let file = h
            .open(0, "/data".into(), OpenFlags::READ, FileAttributes::default())
            .await
            .expect("open")
            .handle;

        let attrs = h.fstat(1, file).await.expect("fstat").attrs;

        assert_eq!(attrs.size, Some(5));
    }

    #[tokio::test]
    async fn exclusive_create_on_existing_file_is_not_no_such_file() {
        let tmp = TempDir::new().expect("tempdir");
        fs::write(tmp.path().join("taken"), b"").expect("write");

        let mut h = handler(&tmp);
        let result = h
            .open(
                0,
                "/taken".into(),
                OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUDE,
                FileAttributes::default(),
            )
            .await;

        assert!(matches!(result, Err(StatusCode::Failure)));
    }

    #[tokio::test]
    async fn open_missing_file_is_no_such_file() {
        let tmp = TempDir::new().expect("tempdir");
        let mut h = handler(&tmp);

        let result = h
            .open(0, "/nope".into(), OpenFlags::READ, FileAttributes::default())
            .await;

        assert!(matches!(result, Err(StatusCode::NoSuchFile)));
    }

    #[test]
    fn longname_formats_recent_file() {
        // 2023-11-14 22:13:20 UTC
        let now = 1_700_000_000;
        let attrs = FileAttr {
            size: Some(1234),
            uid: Some(1000),
            gid: Some(1000),
            permissions: Some(0o100_644),
            atime: None,
            mtime: Some(1_700_000_000),
        };

        assert_eq!(
            longname("hello.txt", &attrs, now),
            "-rw-r--r--   1 1000     1000         1234 Nov 14 22:13 hello.txt"
        );
    }

    #[test]
    fn longname_formats_old_file_with_year() {
        let now = 1_700_000_000;
        let attrs = FileAttr {
            size: Some(0),
            uid: Some(0),
            gid: Some(0),
            permissions: Some(0o040_755),
            atime: None,
            // 2001-09-09 01:46:40 UTC
            mtime: Some(1_000_000_000),
        };

        assert_eq!(
            longname("dir", &attrs, now),
            "drwxr-xr-x   1 0        0               0 Sep  9  2001 dir"
        );
    }

    #[test]
    fn mode_string_covers_file_types() {
        assert_eq!(mode_string(0o100_644), "-rw-r--r--");
        assert_eq!(mode_string(0o040_700), "drwx------");
        assert_eq!(mode_string(0o120_777), "lrwxrwxrwx");
    }
}
