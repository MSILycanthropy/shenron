#![cfg(feature = "sftp")]

use std::fs;

use shenron::sftp::{FileHandle, Filesystem, LocalFilesystem};
use tempfile::TempDir;

fn root_with_file() -> (TempDir, LocalFilesystem) {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("hello.txt"), b"hi there").expect("seed file");

    let fs = LocalFilesystem::new(dir.path());

    (dir, fs)
}

#[tokio::test]
async fn reads_a_file_within_root() {
    let (_dir, fs) = root_with_file();

    let mut handle = fs.open_read("/hello.txt").await.expect("open");
    let data = handle.read(0, 1024).await.expect("read");

    assert_eq!(data, b"hi there");
}

#[tokio::test]
async fn write_then_read_roundtrips() {
    let (_dir, fs) = root_with_file();

    let mut writer = fs
        .open_write(
            "/new.txt",
            russh_sftp::protocol::OpenFlags::CREATE | russh_sftp::protocol::OpenFlags::WRITE,
            shenron::sftp::FileAttr::default(),
        )
        .await
        .expect("open_write");
    writer.write(0, b"payload".to_vec()).await.expect("write");

    let mut reader = fs.open_read("/new.txt").await.expect("open_read");
    assert_eq!(reader.read(0, 1024).await.expect("read"), b"payload");
}

#[tokio::test]
async fn rejects_parent_directory_traversal() {
    let (dir, fs) = root_with_file();

    // A secret sitting outside the served root.
    let secret = dir
        .path()
        .parent()
        .expect("temp dir has a parent")
        .join("shenron-secret.txt");
    fs::write(&secret, b"top secret").expect("seed secret");

    assert!(fs.open_read("/../shenron-secret.txt").await.is_err());
    assert!(fs.stat("/../shenron-secret.txt").await.is_err());
    assert!(
        fs.open_write(
            "/../shenron-secret.txt",
            russh_sftp::protocol::OpenFlags::WRITE,
            shenron::sftp::FileAttr::default(),
        )
        .await
        .is_err()
    );

    let _ = fs::remove_file(secret);
}

#[tokio::test]
async fn realpath_is_virtual_not_host_path() {
    let (_dir, fs) = root_with_file();

    let resolved = fs.realpath("/hello.txt").await.expect("realpath");

    // Must be the path as the client sees it, never the host's real location.
    assert_eq!(resolved, "/hello.txt");
}

#[cfg(unix)]
#[tokio::test]
async fn create_honors_client_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let (dir, fs) = root_with_file();

    let attrs = shenron::sftp::FileAttr {
        permissions: Some(0o600),
        ..Default::default()
    };
    fs.open_write(
        "/secret.key",
        russh_sftp::protocol::OpenFlags::CREATE | russh_sftp::protocol::OpenFlags::WRITE,
        attrs,
    )
    .await
    .expect("open_write");

    let mode = std::fs::metadata(dir.path().join("secret.key"))
        .expect("meta")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[cfg(unix)]
#[tokio::test]
async fn mkdir_honors_client_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let (dir, fs) = root_with_file();

    let attrs = shenron::sftp::FileAttr {
        permissions: Some(0o700),
        ..Default::default()
    };
    fs.mkdir("/private", attrs).await.expect("mkdir");

    let mode = std::fs::metadata(dir.path().join("private"))
        .expect("meta")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700);
}

#[tokio::test]
async fn mkdir_and_rmdir() {
    let (_dir, fs) = root_with_file();

    fs.mkdir("/sub", shenron::sftp::FileAttr::default())
        .await
        .expect("mkdir");
    assert!(fs.stat("/sub").await.is_ok());

    fs.rmdir("/sub").await.expect("rmdir");
    assert!(fs.stat("/sub").await.is_err());
}
