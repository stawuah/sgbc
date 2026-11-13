//! Reviewable deployment actions. Only the display form uses shell quoting;
//! commands retain separate arguments and an explicit execution identity.
use crate::config::Project;
use crate::layout::Layout;
use std::fmt;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identity {
    Root,
    Project(String),
}

#[derive(Debug, Clone)]
pub struct PlannedCommand {
    pub identity: Identity,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

impl fmt::Display for PlannedCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.identity {
            Identity::Root => write!(f, "[root] ")?,
            Identity::Project(user) => write!(f, "[user={user}] ")?,
        }
        if let Some(cwd) = &self.cwd {
            write!(f, "cd {} && ", quote(&cwd.to_string_lossy()))?;
        }
        write!(f, "{}", quote(&self.program))?;
        for arg in &self.args {
            write!(f, " {}", quote(arg))?;
        }
        Ok(())
    }
}

pub struct Step {
    pub summary: String,
    pub command: Option<PlannedCommand>,
}

impl Step {
    fn note(summary: impl Into<String>) -> Self {
        Self {
            summary: summary.into(),
            command: None,
        }
    }
    fn run(
        summary: impl Into<String>,
        identity: Identity,
        cwd: Option<PathBuf>,
        program: &str,
        args: Vec<String>,
    ) -> Self {
        Self {
            summary: summary.into(),
            command: Some(PlannedCommand {
                identity,
                program: program.into(),
                args,
                cwd,
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checkout {
    New,
    Existing,
}

/// Offline template: assumes new checkouts. `deploy::inspect` supplies actual
/// local checkout states for `deploy --dry-run`.
pub fn build(project: &Project, layout: &Layout) -> Vec<Step> {
    build_with_checkouts(
        project,
        layout,
        &vec![Checkout::New; project.services().len()],
    )
}

pub(crate) fn build_with_checkouts(
    project: &Project,
    layout: &Layout,
    checkouts: &[Checkout],
) -> Vec<Step> {
    assert_eq!(checkouts.len(), project.services().len());
    let mut steps = Vec::new();
    let user = layout.service_user();
    let root = Identity::Root;
    let builder = Identity::Project(user.clone());
    steps.push(Step::note(format!("[root prerequisite] Ensure dedicated account/group {user} exists with home {} and no login shell; reject an unrelated existing account", layout.project_dir().display())));
    for dir in layout.platform_directories() {
        steps.push(Step::run(
            "Ensure platform-owned directory",
            root.clone(),
            None,
            "install",
            vec![
                "-d".into(),
                "-o".into(),
                "root".into(),
                "-g".into(),
                "root".into(),
                "-m".into(),
                "755".into(),
                dir.display().to_string(),
            ],
        ));
    }
    for dir in layout.project_directories() {
        steps.push(Step::run(
            "Ensure project-owned directory",
            root.clone(),
            None,
            "install",
            vec![
                "-d".into(),
                "-o".into(),
                user.clone(),
                "-g".into(),
                user.clone(),
                "-m".into(),
                "750".into(),
                dir.display().to_string(),
            ],
        ));
    }
    for ((role, service), checkout) in project.services().into_iter().zip(checkouts) {
        let dir = layout.service_dir(role);
        match checkout {
            Checkout::New => steps.push(Step::run(
                format!("First deployment: clone {role}"),
                builder.clone(),
                None,
                "git",
                vec![
                    "clone".into(),
                    "--depth".into(),
                    "1".into(),
                    "--branch".into(),
                    service.branch_name().into(),
                    "--".into(),
                    service.clone_url(),
                    dir.display().to_string(),
                ],
            )),
            Checkout::Existing => {
                steps.push(Step::run(
                    format!("Update: fetch {role} branch {}", service.branch_name()),
                    builder.clone(),
                    Some(dir.clone()),
                    "git",
                    vec![
                        "fetch".into(),
                        "--no-tags".into(),
                        "origin".into(),
                        format!("refs/heads/{}", service.branch_name()),
                    ],
                ));
                steps.push(Step::run(
                    format!("Update {role} only if fast-forward is possible; stop on divergence"),
                    builder.clone(),
                    Some(dir.clone()),
                    "git",
                    vec!["merge".into(), "--ff-only".into(), "FETCH_HEAD".into()],
                ));
            }
        }
        if let Some(build) = &service.build {
            steps.push(Step::run(
                format!("Build {role} (include dependency installation in this command)"),
                builder.clone(),
                Some(dir),
                "/bin/sh",
                vec!["-c".into(), build.clone()],
            ));
        }
    }
    steps.push(Step::note("[root prerequisite] Render fresh units and Caddy fragment into platform-owned staging; validate units before installation. Do not install stale files."));
    for (role, _) in project.services() {
        steps.push(Step::run(
            format!("Install {role} unit"),
            root.clone(),
            None,
            "install",
            vec![
                "-o".into(),
                "root".into(),
                "-g".into(),
                "root".into(),
                "-m".into(),
                "644".into(),
                layout.unit_path(role).display().to_string(),
                format!("/etc/systemd/system/{}", layout.unit_name(role)),
            ],
        ));
    }
    steps.push(Step::run(
        "Reload systemd",
        root.clone(),
        None,
        "systemctl",
        vec!["daemon-reload".into()],
    ));
    for (role, _) in project.services() {
        steps.push(Step::run(
            format!("Enable {role}"),
            root.clone(),
            None,
            "systemctl",
            vec!["enable".into(), layout.unit_name(role)],
        ));
        steps.push(Step::run(
            format!("Start or restart {role} with the updated code"),
            root.clone(),
            None,
            "systemctl",
            vec!["restart".into(), layout.unit_name(role)],
        ));
    }
    if project
        .services()
        .iter()
        .any(|(role, _)| project.domain_for(*role).is_some())
    {
        steps.push(Step::note("[root prerequisite] Configure Caddy site imports, DNS, and Caddy access to access-log directories; stage and validate the complete candidate Caddy configuration before replacing the active fragment"));
        steps.push(Step::run(
            "Install validated Caddy fragment",
            root.clone(),
            None,
            "install",
            vec![
                "-o".into(),
                "root".into(),
                "-g".into(),
                "root".into(),
                "-m".into(),
                "644".into(),
                layout.caddy_fragment().display().to_string(),
                format!("/etc/caddy/sites/{}.caddy", project.name),
            ],
        ));
        steps.push(Step::run(
            "Reload Caddy",
            root,
            None,
            "systemctl",
            vec!["reload".into(), "caddy".into()],
        ));
    } else {
        steps.push(Step::note(
            "No domain is configured, so Caddy is left untouched",
        ));
    }
    steps.push(Step::note("Execution is not implemented. Stop on any failure. In-place updates are not atomic and have no rollback or application health check."));
    steps
}
