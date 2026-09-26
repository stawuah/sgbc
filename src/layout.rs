//! Where a project's files live on the host.
//!
//! Every path is derived from the project name, which `config::validate_name`
//! has already constrained to lowercase letters, digits and hyphens — so no
//! path here can escape the root.

use crate::config::Role;
use std::path::{Path, PathBuf};

/// The default root on a provisioned host.
pub const DEFAULT_ROOT: &str = "/srv/solanaghana";

#[derive(Debug, Clone)]
pub struct Layout {
    root: PathBuf,
    project: String,
}

impl Layout {
    pub fn new(root: impl AsRef<Path>, project: &str) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            project: project.to_string(),
        }
    }

    /// `<root>/projects/<name>` — everything the project owns.
    pub fn project_dir(&self) -> PathBuf {
        self.root.join("projects").join(&self.project)
    }

    /// `<root>/projects/<name>/<role>` — one checkout per service.
    pub fn service_dir(&self, role: Role) -> PathBuf {
        self.project_dir().join(role.as_str())
    }

    /// `<root>/deployments/<name>` — deploy records, one per attempt.
    pub fn deployments_dir(&self) -> PathBuf {
        self.root.join("deployments").join(&self.project)
    }

    /// `<root>/configs` — generated Caddy fragments, imported by the Caddyfile.
    pub fn configs_dir(&self) -> PathBuf {
        self.root.join("configs")
    }

    pub fn caddy_fragment(&self) -> PathBuf {
        self.configs_dir().join(format!("{}.caddy", self.project))
    }

    /// `<root>/logs` — platform-owned parent of every project's log directory.
    pub fn logs_root(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// `<root>/logs/<name>` — this project's logs, and only this project's.
    /// Each project gets its own directory so one builder cannot read or
    /// truncate another builder's logs.
    pub fn logs_dir(&self) -> PathBuf {
        self.logs_root().join(&self.project)
    }

    pub fn log_file(&self, role: Role) -> PathBuf {
        self.logs_dir().join(format!("{}.log", role.as_str()))
    }

    pub fn access_log_file(&self, role: Role) -> PathBuf {
        self.logs_dir()
            .join(format!("{}.access.log", role.as_str()))
    }

    /// The systemd unit name, without a directory.
    pub fn unit_name(&self, role: Role) -> String {
        format!("{}-{}.service", self.project, role.as_str())
    }

    /// Where the unit is installed. Held under the root so a dry run can write
    /// a complete tree anywhere, and only a privileged deploy targets
    /// /etc/systemd/system.
    pub fn unit_path(&self, role: Role) -> PathBuf {
        self.root.join("systemd").join(self.unit_name(role))
    }

    /// The Linux account the services run as. One account per project, so a
    /// builder's process cannot read another builder's checkout.
    pub fn service_user(&self) -> String {
        self.project.clone()
    }

    pub fn systemd_dir(&self) -> PathBuf {
        self.root.join("systemd")
    }

    /// Shared directories owned by the platform, not by any project. A builder
    /// process must never be given write access to these.
    pub fn platform_directories(&self) -> Vec<PathBuf> {
        vec![
            self.root.join("projects"),
            self.root.join("deployments"),
            self.configs_dir(),
            self.logs_root(),
            self.systemd_dir(),
        ]
    }

    /// Directories owned by this project's service account.
    pub fn project_directories(&self) -> Vec<PathBuf> {
        vec![self.project_dir(), self.deployments_dir(), self.logs_dir()]
    }

    /// Every directory that must exist before anything is written.
    pub fn directories(&self) -> Vec<PathBuf> {
        let mut dirs = self.platform_directories();
        dirs.extend(self.project_directories());
        dirs
    }
}
