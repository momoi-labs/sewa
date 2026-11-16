use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use sewa_approval::Pinned;
use uuid::Uuid;

pub fn default_dir() -> io::Result<PathBuf> {
    if cfg!(target_os = "macos") {
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
const APPROVERS: &str = "approvers.json";

impl State {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

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

    pub fn store_approvers(&self, pinned: &Pinned) -> io::Result<()> {
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.dir)?;
        let json = serde_json::to_vec_pretty(pinned).map_err(io::Error::other)?;
        write_private(&self.dir.join(APPROVERS), &json)
    }

    pub fn approvers(&self) -> Pinned {
        fs::read(self.dir.join(APPROVERS))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn read(&self, name: &str) -> Option<Vec<u8>> {
        fs::read(self.dir.join(name)).ok()
    }

    pub fn write(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.dir)?;
        write_private(&self.dir.join(name), bytes)
    }

    pub fn remove(&self, name: &str) {
        let _ = fs::remove_file(self.dir.join(name));
    }

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

pub fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
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
    fn pins_the_approver_keys() {
        let root = tempfile::tempdir().unwrap();
        let state = State::new(root.path().join("sewa"));
        assert!(state.approvers().keys.is_empty());
        let pinned = Pinned {
            rp_id: "sewa.example".to_owned(),
            origin: "https://sewa.example".to_owned(),
            keys: vec![sewa_approval::testing::Authenticator::new(7).approver("admin@example.com")],
        };
        state.store_approvers(&pinned).unwrap();
        assert_eq!(state.approvers(), pinned);
        assert_eq!(mode(&state.dir.join(APPROVERS)), 0o600);
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
