#![cfg(feature = "sftp")]

use std::fs;

use shenron::sftp::{FileAttr, FileHandle, Filesystem, LocalFilesystem};
use tempfile::TempDir;

/// The served root is a subdirectory of the returned `TempDir`, so escape
/// attempts have somewhere real to land (the outer dir) without touching the
/// shared `/tmp` — parallel-safe, and everything is cleaned up on drop even
/// when an assert fails.
fn sandboxed_root() -> (TempDir, LocalFilesystem) {
    let outer = tempfile::tempdir().expect("tempdir");
    let root = outer.path().join("root");

    fs::create_dir(&root).expect("create root");
    fs::write(root.join("hello.txt"), b"hi there").expect("seed file");

    let fs = LocalFilesystem::new(root);

    (outer, fs)
}

fn root(outer: &TempDir) -> std::path::PathBuf {
    outer.path().join("root")
}

#[tokio::test]
async fn reads_a_file_within_root() {
    let (_outer, fs) = sandboxed_root();

    let mut handle = fs.open_read("/hello.txt").await.expect("open");
    let data = handle.read(0, 1024).await.expect("read");

    assert_eq!(data, b"hi there");
}

#[tokio::test]
async fn write_then_read_roundtrips() {
    let (_outer, fs) = sandboxed_root();

    let mut writer = fs
        .open_write(
            "/new.txt",
            russh_sftp::protocol::OpenFlags::CREATE | russh_sftp::protocol::OpenFlags::WRITE,
            FileAttr::default(),
        )
        .await
        .expect("open_write");
    writer.write(0, b"payload".to_vec()).await.expect("write");

    let mut reader = fs.open_read("/new.txt").await.expect("open_read");
    assert_eq!(reader.read(0, 1024).await.expect("read"), b"payload");
}

#[tokio::test]
async fn reads_and_writes_at_nonzero_offsets() {
    let (_outer, fs) = sandboxed_root();

    let mut writer = fs
        .open_write(
            "/pos.txt",
            russh_sftp::protocol::OpenFlags::CREATE | russh_sftp::protocol::OpenFlags::WRITE,
            FileAttr::default(),
        )
        .await
        .expect("open_write");
    writer
        .write(0, b"hello world".to_vec())
        .await
        .expect("write at 0");
    writer
        .write(6, b"rust!".to_vec())
        .await
        .expect("write at 6");

    let mut reader = fs.open_read("/pos.txt").await.expect("open_read");
    assert_eq!(
        reader.read(0, 1024).await.expect("read all"),
        b"hello rust!"
    );
    assert_eq!(reader.read(6, 5).await.expect("read at 6"), b"rust!");
}

#[tokio::test]
async fn rejects_parent_directory_traversal() {
    let (outer, fs) = sandboxed_root();

    // A secret sitting outside the served root.
    fs::write(outer.path().join("secret.txt"), b"top secret").expect("seed secret");

    assert!(fs.open_read("/../secret.txt").await.is_err());
    assert!(fs.stat("/../secret.txt").await.is_err());
    assert!(
        fs.open_write(
            "/../secret.txt",
            russh_sftp::protocol::OpenFlags::WRITE,
            FileAttr::default(),
        )
        .await
        .is_err()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn rejects_symlinks_escaping_the_root() {
    use std::os::unix::fs::symlink;

    let (outer, fs) = sandboxed_root();

    fs::write(outer.path().join("secret.txt"), b"top secret").expect("seed secret");
    symlink(
        outer.path().join("secret.txt"),
        root(&outer).join("link_abs"),
    )
    .expect("abs link");
    symlink("../secret.txt", root(&outer).join("link_rel")).expect("rel link");

    for link in ["/link_abs", "/link_rel"] {
        assert!(fs.open_read(link).await.is_err(), "{link} should not open");
        assert!(fs.stat(link).await.is_err(), "{link} should not stat");
    }

    // lstat inspects the link itself, not the target — that must still work.
    assert!(fs.lstat("/link_rel").await.is_ok());
}

#[cfg(unix)]
#[tokio::test]
async fn follows_symlinks_within_the_root() {
    use std::os::unix::fs::symlink;

    let (outer, fs) = sandboxed_root();

    symlink("hello.txt", root(&outer).join("inlink")).expect("in-root link");

    let mut handle = fs.open_read("/inlink").await.expect("open through link");
    assert_eq!(handle.read(0, 1024).await.expect("read"), b"hi there");
}

#[tokio::test]
async fn rejects_rename_traversal() {
    let (outer, fs) = sandboxed_root();

    fs::write(outer.path().join("secret.txt"), b"top secret").expect("seed secret");

    // Moving a file out of the root is as bad as reading outside it.
    assert!(fs.rename("/hello.txt", "/../stolen.txt").await.is_err());
    assert!(!outer.path().join("stolen.txt").exists());
    assert!(root(&outer).join("hello.txt").exists());

    // Pulling a file into the root from outside.
    assert!(fs.rename("/../secret.txt", "/grabbed.txt").await.is_err());
    assert!(!root(&outer).join("grabbed.txt").exists());
}

#[tokio::test]
async fn realpath_is_virtual_not_host_path() {
    let (_outer, fs) = sandboxed_root();

    let resolved = fs.realpath("/hello.txt").await.expect("realpath");

    // Must be the path as the client sees it, never the host's real location.
    assert_eq!(resolved, "/hello.txt");
}

#[cfg(unix)]
#[tokio::test]
async fn set_stat_applies_permissions_and_truncates() {
    use std::os::unix::fs::PermissionsExt;

    let (outer, fs) = sandboxed_root();

    fs.set_stat(
        "/hello.txt",
        FileAttr {
            permissions: Some(0o600),
            ..Default::default()
        },
    )
    .await
    .expect("set permissions");

    let meta = fs::metadata(root(&outer).join("hello.txt")).expect("meta");
    assert_eq!(meta.permissions().mode() & 0o777, 0o600);

    fs.set_stat(
        "/hello.txt",
        FileAttr {
            size: Some(2),
            ..Default::default()
        },
    )
    .await
    .expect("truncate");

    assert_eq!(fs.stat("/hello.txt").await.expect("stat").size, Some(2));
}

#[cfg(unix)]
#[tokio::test]
async fn create_honors_client_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let (outer, fs) = sandboxed_root();

    let attrs = FileAttr {
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

    let mode = fs::metadata(root(&outer).join("secret.key"))
        .expect("meta")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[cfg(unix)]
#[tokio::test]
async fn mkdir_honors_client_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let (outer, fs) = sandboxed_root();

    let attrs = FileAttr {
        permissions: Some(0o700),
        ..Default::default()
    };
    fs.mkdir("/private", attrs).await.expect("mkdir");

    let mode = fs::metadata(root(&outer).join("private"))
        .expect("meta")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700);
}

#[tokio::test]
async fn mkdir_and_rmdir() {
    let (_outer, fs) = sandboxed_root();

    fs.mkdir("/sub", FileAttr::default()).await.expect("mkdir");
    assert!(fs.stat("/sub").await.is_ok());

    fs.rmdir("/sub").await.expect("rmdir");
    assert!(fs.stat("/sub").await.is_err());
}
