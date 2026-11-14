//! What the agent keeps on disk: the controller's address, the credential,
//! and the instance UID, in one private directory.
//!
//! - Linux: `/var/lib/sewa`, as root.
//! - macOS: `~/Library/Application Support/sewa` of the user the agent
//!   runs as.

use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use uuid::Uuid;

/// The directory of this system, for the user the agent runs as.
pub fn default_dir() -> io::Result<PathBuf> {
    if cfg!(target_os = "macos") {
        // The home of the running user, not $HOME: launchd and sudo do not
        // always set it.
        let user = nix::unistd::User::from_uid(nix::unistd::getuid())
            .map_err(io::Error::from)?
            .ok_or_else(|| io::Error::other("the running user has no account"))?;
        Ok(user.dir.join("Library/Application Support/sewa"))
    } else {
        Ok(PathBuf::from("/var/lib/sewa"))
    }
}

pub struct State {
    pub dir: PathBuf,
}

const SERVER: &str = "server";
const CREDENTIAL: &str = "credential";
const INSTANCE_UID: &str = "instance_uid";

impl State {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// Stores what enrollment returned, replacing an earlier enrollment.
    pub fn store_enrollment(&self, server: &str, credential: &str) -> io::Result<()> {
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.dir)?;
        write_private(&self.dir.join(SERVER), format!("{server}\n").as_bytes())?;
        write_private(
            &self.dir.join(CREDENTIAL),
            format!("{credential}\n").as_bytes(),
        )
    }

    /// The controller's address and the credential.
    pub fn enrollment(&self) -> io::Result<(String, String)> {
        let read = |name: &str| -> io::Result<String> {
            let path = self.dir.join(name);
            let text = fs::read_to_string(&path).map_err(|error| {
                io::Error::new(error.kind(), format!("{}: {error}", path.display()))
            })?;
            Ok(text.trim().to_owned())
        };
        Ok((read(SERVER)?, read(CREDENTIAL)?))
    }

    /// Reads the instance UID, or creates a UUIDv7 and keeps it, so the
    /// agent keeps its identity across restarts. From pg-yard's
    /// `crates/agent/src/opamp.rs`.
    pub fn instance_uid(&self) -> io::Result<Uuid> {
        let path = self.dir.join(INSTANCE_UID);
        match fs::read_to_string(&path) {
            Ok(text) => {
                if let Ok(uid) = Uuid::try_parse(text.trim()) {
                    return Ok(uid);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let uid = Uuid::now_v7();
        write_private(&path, format!("{uid}\n").as_bytes())?;
        Ok(uid)
    }
}

/// Writes a file, mode 0600, through a temporary file, so a crash never
/// leaves half of one. From pg-yard's `crates/agent/src/opamp.rs`.
fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    let _ = fs::remove_file(&temporary);
    let written = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)
        .and_then(|mut file| {
            file.write_all(bytes)?;
            file.sync_all()
        })
        .and_then(|()| fs::rename(&temporary, path));
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn keeps_the_enrollment_private() {
        let root = tempfile::tempdir().unwrap();
        let state = State::new(root.path().join("sewa"));
        state
            .store_enrollment("http://controller.example:8080", "secret")
            .unwrap();
        assert_eq!(mode(&state.dir), 0o700);
        assert_eq!(mode(&state.dir.join(CREDENTIAL)), 0o600);
        assert_eq!(
            state.enrollment().unwrap(),
            (
                "http://controller.example:8080".to_owned(),
                "secret".to_owned()
            )
        );
    }

    #[test]
    fn keeps_the_instance_uid() {
        let root = tempfile::tempdir().unwrap();
        let state = State::new(root.path().to_path_buf());
        let uid = state.instance_uid().unwrap();
        assert_eq!(state.instance_uid().unwrap(), uid);
        assert_eq!(mode(&root.path().join(INSTANCE_UID)), 0o600);
    }
}
