use std::{
    collections::HashMap,
    sync::atomic::{AtomicU64, Ordering},
};

use russh_sftp::protocol::{
    Attrs, Data, FileAttributes, Handle, Name, OpenFlags, Status, StatusCode, Version,
};

use crate::middleware::builtins::sftp::filesystem::{DirEntry, FileHandle, Filesystem};

/// Internal handler that implements `russh_sftp::server::Handler`
pub struct SftpHandler<F: Filesystem> {
    fs: F,
    handles: HashMap<String, HandleType>,
    next_handle: AtomicU64,

    version: Option<u32>,
}

enum HandleType {
    File(Box<dyn FileHandle>),
    Dir { entries: Vec<DirEntry>, read: bool },
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
            Some(HandleType::File(f)) => f.close().map_err(|_| StatusCode::Failure)?,
            Some(HandleType::Dir { .. }) => {}
            None => return Err(StatusCode::Failure),
        }

        Ok(Status {
            id,
            status_code: StatusCode::Ok,
            error_message: "Ok".to_string(),
            language_tag: "en-US".to_string(),
        })
    }

    async fn open(
        &mut self,
        id: u32,
        filename: String,
        pflags: OpenFlags,
        _attrs: FileAttributes,
    ) -> Result<Handle, Self::Error> {
        let handle = self.next_handle();

        let file = if pflags.contains(OpenFlags::WRITE) || pflags.contains(OpenFlags::CREATE) {
            self.fs.open_write(&filename, pflags)
        } else {
            self.fs.open_read(&filename)
        }
        .map_err(|_| StatusCode::NoSuchFile)?;

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

        let data = f.read(offset, len).map_err(|_| StatusCode::Failure)?;

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

        f.write(offset, &data).map_err(|_| StatusCode::Failure)?;

        Ok(Status {
            id,
            status_code: StatusCode::Ok,
            error_message: "Ok".to_string(),
            language_tag: "en-US".to_string(),
        })
    }

    async fn opendir(&mut self, id: u32, path: String) -> Result<Handle, Self::Error> {
        let entries = self
            .fs
            .read_dir(&path)
            .map_err(|_| StatusCode::NoSuchFile)?;
        let handle = self.next_handle();

        self.handles.insert(
            handle.clone(),
            HandleType::Dir {
                entries,
                read: false,
            },
        );

        Ok(Handle { id, handle })
    }

    async fn readdir(&mut self, id: u32, handle: String) -> Result<Name, Self::Error> {
        let Some(HandleType::Dir { entries, read, .. }) = self.handles.get_mut(&handle) else {
            return Err(StatusCode::Failure);
        };

        if *read {
            return Err(StatusCode::Eof);
        }

        *read = true;

        let files: Vec<_> = entries
            .iter()
            .map(|e| russh_sftp::protocol::File {
                filename: e.name.clone(),
                longname: e.name.clone(),
                attrs: e.attrs.clone().into(),
            })
            .collect();

        Ok(Name { id, files })
    }

    async fn realpath(&mut self, id: u32, path: String) -> Result<Name, Self::Error> {
        let real = self
            .fs
            .realpath(&path)
            .map_err(|_| StatusCode::NoSuchFile)?;

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
        let attrs = self.fs.stat(&path).map_err(|_| StatusCode::NoSuchFile)?;

        Ok(Attrs {
            id,
            attrs: attrs.into(),
        })
    }

    async fn lstat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        let attrs = self.fs.lstat(&path).map_err(|_| StatusCode::NoSuchFile)?;

        Ok(Attrs {
            id,
            attrs: attrs.into(),
        })
    }

    async fn remove(&mut self, id: u32, filename: String) -> Result<Status, Self::Error> {
        self.fs.remove(&filename).map_err(|_| StatusCode::Failure)?;

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
            .map_err(|_| StatusCode::Failure)?;

        status_ok(id)
    }

    async fn rmdir(&mut self, id: u32, path: String) -> Result<Status, Self::Error> {
        self.fs.rmdir(&path).map_err(|_| StatusCode::Failure)?;

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
            .map_err(|_| StatusCode::Failure)?;

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
