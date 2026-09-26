//! Project configuration: the `project.yaml` a builder commits to their repo.
//!
//! Every value here ends up in a systemd unit, a Caddy site block or a
//! filesystem path on shared infrastructure, so parsing is only half the job.
//! The validation below is the trust boundary between a builder's config and
//! the host, and it is deliberately stricter than YAML itself requires.

use anyhow::{Result, bail};
use serde::Deserialize;
use std::fmt;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub name: String,
    #[serde(default)]
    pub frontend: Option<Service>,
    #[serde(default)]
    pub backend: Option<Service>,
    #[serde(default)]
    pub domain: Option<Domains>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Service {
    pub repo: String,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub build: Option<String>,
    #[serde(default)]
    pub start: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Domains {
    #[serde(default)]
    pub frontend: Option<String>,
    #[serde(default)]
    pub api: Option<String>,
}

/// Which slot a service occupies. `Backend` is the generic non-browser slot:
/// an API, a worker, a bot, an indexer or an MCP server all deploy through it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Frontend,
    Backend,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Frontend => "frontend",
            Role::Backend => "backend",
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `pad` honours width and alignment; `write_str` silently ignores them.
        f.pad(self.as_str())
    }
}

impl Project {
    pub fn parse(yaml: &str) -> Result<Self> {
        let project: Project = serde_yaml::from_str(yaml)?;
        project.validate()?;
        Ok(project)
    }

    /// Every configured service, in deploy order.
    pub fn services(&self) -> Vec<(Role, &Service)> {
        let mut out = Vec::new();
        if let Some(service) = &self.backend {
            out.push((Role::Backend, service));
        }
        if let Some(service) = &self.frontend {
            out.push((Role::Frontend, service));
        }
        out
    }

    /// The domain routed to a given service, if one is configured.
    pub fn domain_for(&self, role: Role) -> Option<&str> {
        let domains = self.domain.as_ref()?;
        match role {
            Role::Frontend => domains.frontend.as_deref(),
            Role::Backend => domains.api.as_deref(),
        }
    }

    fn validate(&self) -> Result<()> {
        // The name becomes a directory, a unit name and a Linux user, so it is
        // held to the strictest of those three.
        validate_name(&self.name)?;

        if self.frontend.is_none() && self.backend.is_none() {
            bail!("a project must define at least one of `frontend` or `backend`");
        }

        for (role, service) in self.services() {
            service
                .validate()
                .map_err(|e| anyhow::anyhow!("{role}: {e}"))?;

            if let Some(domain) = self.domain_for(role) {
                validate_hostname(domain)
                    .map_err(|e| anyhow::anyhow!("domain.{}: {e}", domain_key(role)))?;
                if service.port.is_none() {
                    bail!(
                        "{role}: a `port` is required because domain.{} routes to it",
                        domain_key(role)
                    );
                }
            }
        }

        if let (Some(frontend), Some(backend)) = (&self.frontend, &self.backend)
            && let (Some(a), Some(b)) = (frontend.port, backend.port)
            && a == b
        {
            bail!("frontend and backend cannot share port {a}");
        }

        if let Some(domains) = &self.domain {
            if domains.frontend.is_some() && self.frontend.is_none() {
                bail!("domain.frontend is set but no `frontend` service is defined");
            }
            if domains.api.is_some() && self.backend.is_none() {
                bail!("domain.api is set but no `backend` service is defined");
            }
            if let (Some(a), Some(b)) = (&domains.frontend, &domains.api)
                && a == b
            {
                bail!("domain.frontend and domain.api cannot both be {a}");
            }
        }

        Ok(())
    }
}

fn domain_key(role: Role) -> &'static str {
    match role {
        Role::Frontend => "frontend",
        Role::Backend => "api",
    }
}

impl Service {
    /// The command systemd runs, defaulting per ecosystem convention.
    pub fn start_command(&self) -> &str {
        self.start.as_deref().unwrap_or("npm run start")
    }

    pub fn branch_name(&self) -> &str {
        self.branch.as_deref().unwrap_or("main")
    }

    /// `github.com/user/repo` as given, normalised to a clone URL.
    pub fn clone_url(&self) -> String {
        if self.repo.starts_with("https://") || self.repo.starts_with("git@") {
            self.repo.clone()
        } else {
            format!("https://{}", self.repo)
        }
    }

    fn validate(&self) -> Result<()> {
        validate_repo(&self.repo)?;

        if let Some(branch) = &self.branch {
            if branch.is_empty() || branch.len() > 100 {
                bail!("`branch` must be between 1 and 100 characters");
            }
            if branch.contains(|c: char| c.is_whitespace() || c == '\'' || c == ';') {
                bail!("`branch` contains characters that are not valid in a git ref");
            }
        }

        // A newline would let a build command append arbitrary directives to the
        // generated systemd unit, so commands are checked before they are ever
        // written into one.
        for (field, value) in [("build", &self.build), ("start", &self.start)] {
            if let Some(value) = value {
                if value.trim().is_empty() {
                    bail!("`{field}` is set but empty; remove it or give it a command");
                }
                if value.contains('\n') || value.contains('\r') {
                    bail!(
                        "`{field}` must be a single line; a newline would corrupt the generated systemd unit"
                    );
                }
                if value.len() > 500 {
                    bail!("`{field}` must be 500 characters or fewer");
                }
            }
        }

        if let Some(port) = self.port {
            // Ports below 1024 need privileges this platform never grants a
            // builder process.
            if port < 1024 {
                bail!("`port` must be 1024 or above; {port} is privileged");
            }
        }

        Ok(())
    }
}

/// Valid as a directory name, a systemd unit name and a Linux username at once.
pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > 40 {
        bail!("`name` must be between 1 and 40 characters");
    }
    let first = name.chars().next().unwrap();
    if !first.is_ascii_lowercase() {
        bail!("`name` must start with a lowercase letter");
    }
    if name.ends_with('-') {
        bail!("`name` must not end with a hyphen");
    }
    if let Some(bad) = name
        .chars()
        .find(|c| !(c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-'))
    {
        bail!("`name` may only contain lowercase letters, digits and hyphens; found {bad:?}");
    }
    Ok(())
}

fn validate_repo(repo: &str) -> Result<()> {
    if repo.is_empty() || repo.len() > 300 {
        bail!("`repo` must be between 1 and 300 characters");
    }
    // The repo is passed to git, so anything a shell could reinterpret is out.
    if let Some(bad) = repo
        .chars()
        .find(|c| c.is_whitespace() || matches!(c, ';' | '|' | '&' | '$' | '`' | '\'' | '"' | '\\'))
    {
        bail!("`repo` contains a character that is not valid in a git URL: {bad:?}");
    }
    if !repo.contains('/') {
        bail!("`repo` should look like github.com/user/project");
    }
    Ok(())
}

/// A DNS hostname, checked label by label.
pub fn validate_hostname(host: &str) -> Result<()> {
    if host.is_empty() || host.len() > 253 {
        bail!("must be between 1 and 253 characters");
    }
    if host.starts_with('.') || host.ends_with('.') {
        bail!("must not start or end with a dot");
    }
    if !host.contains('.') {
        bail!("must be a fully qualified name, such as project.solanaghana.dev");
    }
    for label in host.split('.') {
        if label.is_empty() {
            bail!("contains an empty label");
        }
        if label.len() > 63 {
            bail!("label {label:?} is longer than 63 characters");
        }
        if label.starts_with('-') || label.ends_with('-') {
            bail!("label {label:?} must not start or end with a hyphen");
        }
        if let Some(bad) = label
            .chars()
            .find(|c| !(c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-'))
        {
            bail!("label {label:?} contains an invalid character {bad:?}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = "name: demo\nbackend:\n  repo: github.com/user/demo\n";

    fn with(extra: &str) -> String {
        format!("{MINIMAL}{extra}")
    }

    #[test]
    fn accepts_a_minimal_project() {
        let project = Project::parse(MINIMAL).unwrap();
        assert_eq!(project.name, "demo");
        assert_eq!(project.services().len(), 1);
    }

    #[test]
    fn requires_at_least_one_service() {
        let error = Project::parse("name: demo\n").unwrap_err().to_string();
        assert!(error.contains("at least one"), "{error}");
    }

    #[test]
    fn rejects_a_name_that_would_escape_the_project_root() {
        for name in ["../etc", "a/b", "demo/../other"] {
            let yaml = format!("name: {name}\nbackend:\n  repo: github.com/u/r\n");
            assert!(
                Project::parse(&yaml).is_err(),
                "{name:?} should be rejected as a project name"
            );
        }
    }

    #[test]
    fn rejects_a_name_that_is_not_a_valid_unit_or_user_name() {
        for name in ["Demo", "9demo", "demo-", "de mo", "demo_x", ""] {
            let yaml = format!("name: {name}\nbackend:\n  repo: github.com/u/r\n");
            assert!(
                Project::parse(&yaml).is_err(),
                "{name:?} should be rejected"
            );
        }
    }

    #[test]
    fn rejects_a_newline_in_a_command_that_would_inject_unit_directives() {
        let yaml = "name: demo\nbackend:\n  repo: github.com/u/r\n  build: \"make\\nExecStartPre=/bin/su root\"\n";
        let error = Project::parse(yaml).unwrap_err().to_string();
        assert!(error.contains("single line"), "{error}");
    }

    #[test]
    fn rejects_shell_metacharacters_in_a_repo() {
        for repo in [
            "github.com/u/r; rm -rf /",
            "github.com/u/$(whoami)",
            "github.com/u/r`id`",
        ] {
            let yaml = format!("name: demo\nbackend:\n  repo: \"{repo}\"\n");
            assert!(
                Project::parse(&yaml).is_err(),
                "{repo:?} should be rejected"
            );
        }
    }

    #[test]
    fn rejects_a_privileged_port() {
        let error = Project::parse(&with("  port: 80\n"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("privileged"), "{error}");
    }

    #[test]
    fn rejects_two_services_sharing_a_port() {
        let yaml = "name: demo\nbackend:\n  repo: github.com/u/a\n  port: 3000\nfrontend:\n  repo: github.com/u/b\n  port: 3000\n";
        let error = Project::parse(yaml).unwrap_err().to_string();
        assert!(error.contains("share port"), "{error}");
    }

    #[test]
    fn rejects_a_routed_service_with_no_port() {
        let yaml = "name: demo\nbackend:\n  repo: github.com/u/r\ndomain:\n  api: api.demo.solanaghana.dev\n";
        let error = Project::parse(yaml).unwrap_err().to_string();
        assert!(error.contains("`port` is required"), "{error}");
    }

    #[test]
    fn rejects_a_domain_with_no_matching_service() {
        let yaml = "name: demo\nbackend:\n  repo: github.com/u/r\n  port: 3000\ndomain:\n  frontend: demo.solanaghana.dev\n";
        let error = Project::parse(yaml).unwrap_err().to_string();
        assert!(error.contains("no `frontend` service"), "{error}");
    }

    #[test]
    fn rejects_an_invalid_hostname() {
        for host in [
            "-bad.solanaghana.dev",
            "bad-.solanaghana.dev",
            "nodot",
            "UPPER.solanaghana.dev",
            "a..b",
        ] {
            let yaml = format!(
                "name: demo\nbackend:\n  repo: github.com/u/r\n  port: 3000\ndomain:\n  api: \"{host}\"\n"
            );
            assert!(
                Project::parse(&yaml).is_err(),
                "{host:?} should be rejected"
            );
        }
    }

    #[test]
    fn rejects_an_unknown_field_so_typos_are_not_silently_ignored() {
        let yaml = "name: demo\nbackend:\n  repo: github.com/u/r\n  prot: 3000\n";
        assert!(Project::parse(yaml).is_err());
    }

    #[test]
    fn defaults_the_branch_and_start_command() {
        let project = Project::parse(MINIMAL).unwrap();
        let (_, service) = project.services()[0];
        assert_eq!(service.branch_name(), "main");
        assert_eq!(service.start_command(), "npm run start");
    }

    #[test]
    fn normalises_a_bare_repo_path_into_a_clone_url() {
        let project = Project::parse(MINIMAL).unwrap();
        let (_, service) = project.services()[0];
        assert_eq!(service.clone_url(), "https://github.com/user/demo");
    }

    #[test]
    fn keeps_an_explicit_clone_url_as_given() {
        let yaml = "name: demo\nbackend:\n  repo: git@github.com:user/demo.git\n";
        let project = Project::parse(yaml).unwrap();
        let (_, service) = project.services()[0];
        assert_eq!(service.clone_url(), "git@github.com:user/demo.git");
    }

    #[test]
    fn deploys_the_backend_before_the_frontend() {
        let yaml =
            "name: demo\nfrontend:\n  repo: github.com/u/a\nbackend:\n  repo: github.com/u/b\n";
        let project = Project::parse(yaml).unwrap();
        let roles: Vec<Role> = project.services().into_iter().map(|(r, _)| r).collect();
        assert_eq!(roles, vec![Role::Backend, Role::Frontend]);
    }
}
