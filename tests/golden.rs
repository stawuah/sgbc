//! Golden tests over the example configurations.
//!
//! These pin the exact text installed onto a host. A diff here means the unit
//! or site block a builder's service runs under has changed, which is a change
//! worth reading rather than approving by reflex.
//!
//! Regenerate deliberately, then review the diff:
//!
//! ```text
//! UPDATE_GOLDEN=1 cargo test
//! ```

use sgbc::config::Project;
use sgbc::layout::{DEFAULT_ROOT, Layout};
use sgbc::{plan, render};
use std::fs;
use std::path::{Path, PathBuf};

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn check(name: &str, actual: &str) {
    let path = golden_dir().join(name);

    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, actual).unwrap();
        return;
    }

    let expected = fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "missing golden file {}. Run `UPDATE_GOLDEN=1 cargo test` to create it.",
            path.display()
        )
    });

    assert_eq!(
        actual,
        expected,
        "\n{} is out of date. Review the change, then run `UPDATE_GOLDEN=1 cargo test`.\n",
        path.display()
    );
}

fn load(example: &str) -> Project {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join(example);
    let yaml = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()));
    Project::parse(&yaml).unwrap_or_else(|e| panic!("{} is invalid: {e:#}", path.display()))
}

fn snapshot(example: &str, stem: &str) {
    let project = load(example);
    let layout = Layout::new(DEFAULT_ROOT, &project.name);

    for (role, service) in project.services() {
        check(
            &format!("{stem}-{role}.service"),
            &render::systemd_unit(&project, service, role, &layout),
        );
    }

    check(
        &format!("{stem}.caddy"),
        &render::caddy_fragment(&project, &layout),
    );

    let steps = plan::build(&project, &layout);
    let mut rendered = String::new();
    for (index, step) in steps.iter().enumerate() {
        rendered.push_str(&format!("{:>3}. {}\n", index + 1, step.summary));
        if let Some(command) = &step.command {
            rendered.push_str(&format!("     $ {command}\n"));
        }
    }
    check(&format!("{stem}.plan"), &rendered);
}

#[test]
fn frontend_and_backend_project() {
    snapshot("project.yaml", "project");
}

#[test]
fn worker_with_no_domain() {
    snapshot("worker.yaml", "worker");
}

/// The generated paths must stay inside the configured root. The name is
/// already constrained, but this asserts the property the constraint exists for.
#[test]
fn every_generated_path_stays_under_the_root() {
    for example in ["project.yaml", "worker.yaml"] {
        let project = load(example);
        let root = Path::new("/srv/solanaghana");
        let layout = Layout::new(root, &project.name);

        let mut paths = layout.directories();
        paths.push(layout.caddy_fragment());
        for (role, _) in project.services() {
            paths.push(layout.service_dir(role));
            paths.push(layout.unit_path(role));
            paths.push(layout.log_file(role));
            paths.push(layout.access_log_file(role));
        }

        for path in paths {
            assert!(
                path.starts_with(root),
                "{} escapes the root",
                path.display()
            );
            assert!(
                !path.components().any(|c| c.as_os_str() == ".."),
                "{} contains a parent traversal",
                path.display()
            );
        }
    }
}
