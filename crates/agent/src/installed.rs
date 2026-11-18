//! Where the `keefast-agent` command is for Agents to run, and keeping it there for an
//! AppImage (spec story 83).
//!
//! keefast-agent ships next to keefast in every package. An AppImage, though, is mounted
//! somewhere new each run, so an Agent's config naming the copy inside it would stop working:
//! the desktop app copies it out to keefast's data directory each time it starts, as it does
//! the browser's native host, and Agents are pointed at that copy.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The command's file name, without the platform's executable extension.
pub const AGENT_BINARY: &str = "keefast-agent";

/// The `keefast-agent` an Agent runs, and where it comes from when that place doesn't last.
#[derive(Debug, Clone)]
pub struct AgentCommand {
    path: PathBuf,
    source: Option<PathBuf>,
}

impl AgentCommand {
    /// The command at `path`, installed there by a package.
    pub fn new(path: PathBuf) -> Self {
        Self { path, source: None }
    }

    /// The command kept at `path` as a copy of `source`, refreshed by [`Self::install`].
    pub fn copied_from(source: PathBuf, path: PathBuf) -> Self {
        Self {
            path,
            source: Some(source),
        }
    }

    /// The one shipped next to the running app, or, in an AppImage, its copy in keefast's data
    /// directory (`~/.local/share/keefast/keefast-agent`).
    pub fn for_current_user() -> Option<Self> {
        let app = std::env::current_exe().ok()?;
        let shipped = app
            .with_file_name(AGENT_BINARY)
            .with_extension(std::env::consts::EXE_EXTENSION);
        if let Some(mount) = std::env::var_os("APPDIR")
            && app.starts_with(mount)
        {
            let copy = dirs::data_dir()?.join("keefast").join(AGENT_BINARY);
            return Some(Self::copied_from(shipped, copy));
        }
        Some(Self::new(shipped))
    }

    /// Where Agents find the command.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Puts the command where Agents find it, if it is shipped somewhere else: a copy, replaced
    /// whole, so an Agent running the old one meanwhile is undisturbed. Unchanged copies are
    /// left alone.
    pub fn install(&self) -> io::Result<()> {
        let Some(source) = &self.source else {
            return Ok(());
        };
        let contents = fs::read(source)?;
        if fs::read(&self.path).is_ok_and(|installed| installed == contents) {
            return Ok(());
        }
        let dir = self.path.parent().expect("the command is in a directory");
        fs::create_dir_all(dir)?;
        let staged = self.path.with_extension("new");
        fs::write(&staged, &contents)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&staged, fs::Permissions::from_mode(0o755))?;
        }
        fs::rename(&staged, &self.path)
    }
}
