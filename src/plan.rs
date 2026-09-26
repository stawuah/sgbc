//! The ordered steps a deploy performs.
//!
//! Phase 1 produces this plan and the files it refers to; it does not run the
//! commands. Printing the exact sequence first is what makes the deploy
//! reviewable before the platform is ever given a host to act on.

use crate::config::{Project, Role};
use crate::layout::Layout;

pub struct Step {
    /// What this accomplishes, in a builder's terms.
    pub summary: String,
    /// The command a Phase 2 deploy will run, when the step is a command.
    pub command: Option<String>,
}

impl Step {
    fn note(summary: impl Into<String>) -> Self {
        Self {
            summary: summary.into(),
            command: None,
        }
    }

    fn run(summary: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            summary: summary.into(),
            command: Some(command.into()),
        }
    }
}

pub fn build(project: &Project, layout: &Layout) -> Vec<Step> {
    let mut steps = Vec::new();
    let user = layout.service_user();

    steps.push(Step::run(
        format!("Create the {user} service account, with no login shell"),
        format!(
            "useradd --system --home {} --shell /usr/sbin/nologin {user}",
            layout.project_dir().display()
        ),
    ));

    // Shared directories stay root-owned. Only the project's own subtree is
    // handed to its service account, so one builder cannot reach another's
    // checkout, deploy records or logs.
    for dir in layout.platform_directories() {
        steps.push(Step::run(
            format!("Ensure the platform directory {} exists", dir.display()),
            format!("install -d -o root -g root -m 755 {}", dir.display()),
        ));
    }
    for dir in layout.project_directories() {
        steps.push(Step::run(
            format!("Ensure {} exists, owned by {user}", dir.display()),
            format!("install -d -o {user} -g {user} -m 750 {}", dir.display()),
        ));
    }

    for (role, service) in project.services() {
        let dir = layout.service_dir(role);
        steps.push(Step::run(
            format!("Fetch the {role} repository at {}", service.branch_name()),
            format!(
                "git clone --depth 1 --branch {} {} {}",
                service.branch_name(),
                service.clone_url(),
                dir.display()
            ),
        ));

        if let Some(build) = &service.build {
            steps.push(Step::run(
                format!("Build the {role}"),
                format!("cd {} && {build}", dir.display()),
            ));
        } else {
            steps.push(Step::note(format!(
                "No build command for the {role}; its repository is deployed as fetched"
            )));
        }

        steps.push(Step::run(
            format!("Install the {role} systemd unit"),
            format!(
                "install -o root -g root -m 644 {} /etc/systemd/system/{}",
                layout.unit_path(role).display(),
                layout.unit_name(role)
            ),
        ));
    }

    steps.push(Step::run(
        "Reload systemd so the new units are visible",
        "systemctl daemon-reload",
    ));

    for (role, _) in project.services() {
        steps.push(Step::run(
            format!("Enable and start the {role}"),
            format!("systemctl enable --now {}", layout.unit_name(role)),
        ));
    }

    let routed: Vec<Role> = project
        .services()
        .into_iter()
        .filter(|(role, service)| project.domain_for(*role).is_some() && service.port.is_some())
        .map(|(role, _)| role)
        .collect();

    if routed.is_empty() {
        steps.push(Step::note(
            "No domain is configured, so Caddy is left untouched",
        ));
    } else {
        steps.push(Step::run(
            "Install the Caddy site configuration",
            format!(
                "install -o root -g root -m 644 {} /etc/caddy/sites/{}.caddy",
                layout.caddy_fragment().display(),
                project.name
            ),
        ));
        steps.push(Step::run(
            "Reload Caddy, which provisions HTTPS for each new name",
            "systemctl reload caddy",
        ));
        for role in routed {
            if let Some(domain) = project.domain_for(role) {
                steps.push(Step::note(format!(
                    "Point a DNS record for {domain} at this host, then it serves the {role}"
                )));
            }
        }
    }

    steps
}
