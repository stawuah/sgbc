//! Read-only local preflight. This does not inspect a remote VPS or execute
//! any planned action. Git is used only to read existing checkout metadata.
use crate::{
    config::Project,
    layout::Layout,
    plan::{self, Checkout, Step},
};
use anyhow::{Context, Result, bail};
use std::{
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
};

pub fn absolute_root(root: &Path) -> Result<PathBuf> {
    let root = std::path::absolute(root)?;
    // These paths also become unquoted systemd/Caddy configuration values.
    let value = root.to_str().context("root must be valid UTF-8")?;
    if !value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/-_.".contains(c))
        || root.components().any(|c| matches!(c, Component::ParentDir))
        || root == Path::new("/")
    {
        bail!(
            "root must be a non-root path using only letters, digits, /, -, _, and . with no parent traversal"
        );
    }
    Ok(root)
}

fn reject_symlinks(path: &Path) -> Result<()> {
    let mut current = PathBuf::new();
    for part in path.components() {
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => {
                bail!("refusing symlink {}", current.display())
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
            Err(e) => return Err(e).with_context(|| format!("inspect {}", current.display())),
        }
    }
    Ok(())
}

fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args([
            "--no-optional-locks",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.untrackedCache=false",
            "-C",
        ])
        .arg(dir)
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .context("could not run git for read-only checkout inspection")?;
    if !output.status.success() {
        bail!(
            "cannot inspect {}: git {}: {}",
            dir.display(),
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8(output.stdout)?.trim_end().to_string())
}

pub fn inspect(project: &Project, layout: &Layout) -> Result<Vec<Step>> {
    let mut checkouts = Vec::new();
    for (role, service) in project.services() {
        if let Some(file) = &service.env_file {
            let path = layout.env_file(file);
            reject_symlinks(&path)?;
            let metadata = fs::metadata(&path).with_context(|| {
                format!(
                    "{role}: required env_file {} is missing or inaccessible",
                    path.display()
                )
            })?;
            if !metadata.is_file() {
                bail!("{role}: env_file must be a regular file");
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = metadata.permissions().mode();
                if mode & 0o077 != 0 || mode & 0o400 == 0 {
                    bail!(
                        "{role}: env_file must be owner-readable with no group/other permissions (use chmod 600)"
                    );
                }
            }
        }
        let dir = layout.service_dir(role);
        reject_symlinks(&dir)?;
        let state = if !dir.try_exists()? {
            Checkout::New
        } else {
            if !dir.is_dir() {
                bail!("{} is not a directory", dir.display());
            }
            if fs::read_dir(&dir)?.next().is_none() {
                Checkout::New // Compatible with directories created by `sgbc write`.
            } else {
                if !dir.join(".git").is_dir() {
                    bail!(
                        "{} must be an ordinary Git checkout or an empty directory",
                        dir.display()
                    );
                }
                reject_symlinks(&dir.join(".git"))?;
                if git(&dir, &["rev-parse", "--show-toplevel"])?
                    != fs::canonicalize(&dir)?.to_string_lossy()
                {
                    bail!("{role}: checkout root does not match its service directory");
                }
                if !git(&dir, &["status", "--porcelain", "--untracked-files=all"])?.is_empty() {
                    bail!(
                        "{role}: checkout has local changes; commit or move them before deployment"
                    );
                }
                if git(&dir, &["symbolic-ref", "--quiet", "--short", "HEAD"])?
                    != service.branch_name()
                {
                    bail!(
                        "{role}: checkout branch does not match configured branch {}",
                        service.branch_name()
                    );
                }
                if git(&dir, &["remote", "get-url", "origin"])? != service.clone_url() {
                    bail!("{role}: origin does not match configured repository");
                }
                Checkout::Existing
            }
        };
        checkouts.push(state);
    }
    Ok(plan::build_with_checkouts(project, layout, &checkouts))
}
