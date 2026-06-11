use std::path::Path;

use russh::keys::{Algorithm, PrivateKey, decode_secret_key, load_secret_key, ssh_key::LineEnding};

use crate::{Error, Result};

/// How a generated or loaded host key should be produced.
///
/// The `algorithm` only affects keys that are *generated*; loading an existing
/// key uses whatever algorithm it was written with. A `passphrase` is used both
/// to decrypt an existing key on load and to encrypt a freshly generated key
/// before it is written to disk.
#[derive(Clone)]
pub struct HostKeyOptions {
    algorithm: Algorithm,
    passphrase: Option<String>,
}

impl Default for HostKeyOptions {
    fn default() -> Self {
        Self {
            algorithm: Algorithm::Ed25519,
            passphrase: None,
        }
    }
}

impl HostKeyOptions {
    #[must_use]
    pub const fn new(algorithm: Algorithm) -> Self {
        Self {
            algorithm,
            passphrase: None,
        }
    }

    #[must_use]
    pub fn passphrase(mut self, passphrase: impl Into<String>) -> Self {
        self.passphrase = Some(passphrase.into());
        self
    }
}

/// Load the host key at `path`, generating and persisting one if it is missing.
pub fn load_or_generate(path: &Path, options: HostKeyOptions) -> Result<PrivateKey> {
    let HostKeyOptions {
        algorithm,
        passphrase,
    } = options;

    if path.exists() {
        return Ok(load_secret_key(path, passphrase.as_deref())?);
    }

    match generate_and_persist(path, algorithm, passphrase.as_deref()) {
        // Another process generated the key between our existence check and
        // the create-new write; theirs won, use it.
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            Ok(load_secret_key(path, passphrase.as_deref())?)
        }
        result => result,
    }
}

/// Decode a host key from raw OpenSSH/PEM bytes (Wish `WithHostKeyPEM`).
pub fn from_pem(bytes: &[u8], passphrase: Option<&str>) -> Result<PrivateKey> {
    let pem = std::str::from_utf8(bytes).map_err(|e| Error::Config(e.to_string()))?;

    Ok(decode_secret_key(pem, passphrase)?)
}

/// Generate a fresh host key of `opts.algorithm`, write it to `path` (and its
/// public half to `<path>.pub`), and return it. The private key is created
/// `0o600`; directories this call *creates* are `0o700`, pre-existing ones
/// keep their permissions. Fails with `AlreadyExists` if the key file appeared
/// since the caller's existence check. When a passphrase is set, only the
/// on-disk copy is encrypted; the returned key stays usable by the running
/// server.
fn generate_and_persist(
    path: &Path,
    algorithm: Algorithm,
    passphrase: Option<&str>,
) -> Result<PrivateKey> {
    let key = PrivateKey::random(&mut rand::rng(), algorithm)?;

    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        let mut dirs = std::fs::DirBuilder::new();
        dirs.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut dirs, 0o700);

        dirs.create(parent)?;
    }

    let on_disk = match passphrase {
        Some(passphrase) => key.encrypt(&mut rand::rng(), passphrase)?,
        None => key.clone(),
    };

    let pem = on_disk.to_openssh(LineEnding::LF)?;

    let mut open = std::fs::OpenOptions::new();
    open.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut open, 0o600);

    std::io::Write::write_all(&mut open.open(path)?, pem.as_bytes())?;

    // Wish writes `<path>.pub`; append rather than replace any extension.
    let mut pub_path = path.as_os_str().to_owned();
    pub_path.push(".pub");
    key.public_key().write_openssh_file(Path::new(&pub_path))?;

    tracing::info!(
        "Generated {} host key at {}",
        key.algorithm(),
        path.display()
    );

    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use russh::keys::EcdsaCurve;

    fn temp_path(name: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(name);
        (dir, path)
    }

    #[test]
    fn generates_chosen_algorithm_and_persists() {
        let (_dir, path) = temp_path("id_ecdsa");
        let algo = Algorithm::Ecdsa {
            curve: EcdsaCurve::NistP384,
        };

        let key = load_or_generate(&path, HostKeyOptions::new(algo.clone())).expect("generate");
        assert_eq!(key.algorithm(), algo);
        assert!(path.exists());
        assert!(path.with_extension("pub").exists());

        // Second call loads the persisted key rather than regenerating.
        let reloaded = load_or_generate(&path, HostKeyOptions::new(algo)).expect("reload");
        assert_eq!(reloaded.public_key(), key.public_key());
    }

    #[test]
    fn passphrase_encrypts_on_disk_and_decrypts_on_load() {
        let (_dir, path) = temp_path("id_ed25519");
        let opts = HostKeyOptions::default().passphrase("hunter2");

        let key = load_or_generate(&path, opts).expect("generate");

        // Without the passphrase the on-disk key cannot be read.
        assert!(load_secret_key(&path, None).is_err());

        let decrypted = load_secret_key(&path, Some("hunter2")).expect("decrypt");
        assert_eq!(decrypted.public_key(), key.public_key());
    }

    #[cfg(unix)]
    #[test]
    fn existing_parent_dir_keeps_its_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755))
            .expect("chmod");

        load_or_generate(&dir.path().join("key"), HostKeyOptions::default()).expect("generate");

        let mode = std::fs::metadata(dir.path())
            .expect("meta")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);
    }

    #[cfg(unix)]
    #[test]
    fn created_parent_dir_is_private() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let sub = dir.path().join("sub");

        load_or_generate(&sub.join("key"), HostKeyOptions::default()).expect("generate");

        let mode = std::fs::metadata(&sub).expect("meta").permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
    }

    #[test]
    fn generate_against_existing_file_does_not_clobber() {
        let (_dir, path) = temp_path("key");
        std::fs::write(&path, b"sentinel").expect("write");

        let result = generate_and_persist(&path, Algorithm::Ed25519, None);

        assert!(matches!(
            result,
            Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::AlreadyExists
        ));
        assert_eq!(std::fs::read(&path).expect("read"), b"sentinel");
    }

    #[test]
    fn from_pem_round_trips() {
        let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("random key");
        let pem = key.to_openssh(LineEnding::LF).expect("encode");

        let decoded = from_pem(pem.as_bytes(), None).expect("decode");
        assert_eq!(decoded.public_key(), key.public_key());
    }
}
