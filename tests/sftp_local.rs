#![cfg(feature = "sftp")]

use std::fs;

use shenron::sftp::{Filesystem, LocalFilesystem};
use tempfile::TempDir;

fn root_with_file() -> (TempDir, LocalFilesystem) {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("hello.txt"), b"hi there").expect("seed file");

    let fs = LocalFilesystem::new(dir.path());

    (dir, fs)
}

#[test]
fn reads_a_file_within_root() {
    let (_dir, fs) = root_with_file();

    let mut handle = fs.open_read("/hello.txt").expect("open");
    let data = handle.read(0, 1024).expect("read");

    assert_eq!(data, b"hi there");
}

#[test]
fn write_then_read_roundtrips() {
    let (_dir, fs) = root_with_file();

    let mut writer = fs
        .open_write(
            "/new.txt",
            russh_sftp::protocol::OpenFlags::CREATE | russh_sftp::protocol::OpenFlags::WRITE,
            shenron::sftp::FileAttr::default(),
        )
        .expect("open_write");
    writer.write(0, b"payload").expect("write");

    let mut reader = fs.open_read("/new.txt").expect("open_read");
    assert_eq!(reader.read(0, 1024).expect("read"), b"payload");
}

#[test]
fn rejects_parent_directory_traversal() {
    let (dir, fs) = root_with_file();

    // A secret sitting outside the served root.
    let secret = dir
        .path()
        .parent()
        .expect("temp dir has a parent")
        .join("shenron-secret.txt");
    fs::write(&secret, b"top secret").expect("seed secret");

    assert!(fs.open_read("/../shenron-secret.txt").is_err());
    assert!(fs.stat("/../shenron-secret.txt").is_err());
    assert!(
        fs.open_write(
            "/../shenron-secret.txt",
            russh_sftp::protocol::OpenFlags::WRITE,
            shenron::sftp::FileAttr::default(),
        )
        .is_err()
    );

    let _ = fs::remove_file(secret);
}

#[test]
fn realpath_is_virtual_not_host_path() {
    let (_dir, fs) = root_with_file();

    let resolved = fs.realpath("/hello.txt").expect("realpath");

    // Must be the path as the client sees it, never the host's real location.
    assert_eq!(resolved, "/hello.txt");
}

#[cfg(unix)]
#[test]
fn create_honors_client_permissions() {
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
    .expect("open_write");

    let mode = std::fs::metadata(dir.path().join("secret.key"))
        .expect("meta")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[cfg(unix)]
#[test]
fn mkdir_honors_client_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let (dir, fs) = root_with_file();

    let attrs = shenron::sftp::FileAttr {
        permissions: Some(0o700),
        ..Default::default()
    };
    fs.mkdir("/private", attrs).expect("mkdir");

    let mode = std::fs::metadata(dir.path().join("private"))
        .expect("meta")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700);
}

#[test]
fn mkdir_and_rmdir() {
    let (_dir, fs) = root_with_file();

    fs.mkdir("/sub", shenron::sftp::FileAttr::default())
        .expect("mkdir");
    assert!(fs.stat("/sub").is_ok());

    fs.rmdir("/sub").expect("rmdir");
    assert!(fs.stat("/sub").is_err());
}
