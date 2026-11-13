use sgbc::{
    config::Project,
    deploy,
    layout::Layout,
    plan::{Identity, Step},
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "sgbc-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self { root }
    }
    fn project(&self) -> Project {
        Project::parse("name: demo\nbackend:\n  repo: https://example.invalid/team/app\n  build: sh build.sh\n").unwrap()
    }
    fn layout(&self) -> Layout {
        Layout::new(self.root.join("host"), "demo")
    }
    fn source(&self) -> PathBuf {
        let source = self.root.join("source");
        fs::create_dir(&source).unwrap();
        git(&source, &["init", "-b", "main"]);
        git(&source, &["config", "user.name", "Test"]);
        git(&source, &["config", "user.email", "test@example.invalid"]);
        fs::write(source.join("build.sh"), "cat version > result\n").unwrap();
        fs::write(source.join(".gitignore"), "result\n").unwrap();
        fs::write(source.join("version"), "one\n").unwrap();
        git(&source, &["add", "."]);
        git(&source, &["commit", "-m", "initial"]);
        source
    }
    fn checkout(&self) -> PathBuf {
        let source = self.source();
        let dir = self.layout().service_dir(sgbc::config::Role::Backend);
        fs::create_dir_all(dir.parent().unwrap()).unwrap();
        git(
            &self.root,
            &["clone", source.to_str().unwrap(), dir.to_str().unwrap()],
        );
        git(
            &dir,
            &[
                "remote",
                "set-url",
                "origin",
                "https://example.invalid/team/app",
            ],
        );
        dir
    }
    fn cli(&self, extra: &[&str]) -> Output {
        let config = self.root.join("project.yaml");
        fs::write(
            &config,
            "name: demo\nbackend:\n  repo: https://example.invalid/team/app\n",
        )
        .unwrap();
        Command::new(env!("CARGO_BIN_EXE_sgbc"))
            .arg("deploy")
            .arg(config)
            .arg("--root")
            .arg(self.root.join("host"))
            .args(extra)
            .output()
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn git(dir: &Path, args: &[&str]) {
    let result = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
fn error(f: &Fixture) -> String {
    match deploy::inspect(&f.project(), &f.layout()) {
        Ok(_) => panic!("expected inspection to fail"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn dry_run_does_not_write_or_execute_and_requires_flag() {
    let f = Fixture::new();
    let output = f.cli(&["--dry-run"]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("First deployment: clone backend"));
    assert!(stdout.contains("[user=demo]"));
    assert!(stdout.contains("[root]"));
    assert!(!f.root.join("host").exists());
    let output = f.cli(&[]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--dry-run"));
    assert!(!f.root.join("host").exists());
}

#[test]
fn existing_checkout_is_inspected_without_modification() {
    let f = Fixture::new();
    let dir = f.checkout();
    let index = fs::read(dir.join(".git/index")).unwrap();
    let head = fs::read(dir.join(".git/refs/heads/main")).unwrap();
    let output = f.cli(&["--dry-run"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Update: fetch backend"));
    assert_eq!(fs::read(dir.join(".git/index")).unwrap(), index);
    assert_eq!(fs::read(dir.join(".git/refs/heads/main")).unwrap(), head);
    assert!(!dir.join(".git/FETCH_HEAD").exists());
    assert!(!f.root.join("host/systemd").exists());
}

#[test]
fn refuses_dirty_checkout() {
    let f = Fixture::new();
    let dir = f.checkout();
    fs::write(dir.join("version"), "local edits").unwrap();
    assert!(error(&f).contains("local changes"));
}
#[test]
fn refuses_untracked_files() {
    let f = Fixture::new();
    let dir = f.checkout();
    fs::write(dir.join("untracked"), "keep me").unwrap();
    assert!(error(&f).contains("local changes"));
}
#[test]
fn refuses_wrong_origin_and_branch_and_detached_head() {
    let f = Fixture::new();
    let dir = f.checkout();
    git(
        &dir,
        &[
            "remote",
            "set-url",
            "origin",
            "https://example.invalid/other/repo",
        ],
    );
    assert!(error(&f).contains("origin"));
    git(
        &dir,
        &[
            "remote",
            "set-url",
            "origin",
            "https://example.invalid/team/app",
        ],
    );
    git(&dir, &["checkout", "-b", "other"]);
    assert!(error(&f).contains("branch"));
    git(&dir, &["checkout", "--detach"]);
    assert!(error(&f).contains("cannot inspect"));
}
#[test]
fn empty_directory_is_new_but_non_repository_is_rejected() {
    let f = Fixture::new();
    let dir = f.layout().service_dir(sgbc::config::Role::Backend);
    fs::create_dir_all(&dir).unwrap();
    assert!(
        deploy::inspect(&f.project(), &f.layout())
            .unwrap()
            .iter()
            .any(|s| s.summary.contains("First deployment"))
    );
    fs::write(dir.join("important"), "keep me").unwrap();
    assert!(error(&f).contains("ordinary Git checkout"));
}
#[cfg(unix)]
#[test]
fn refuses_symlinked_checkout_ancestors() {
    let f = Fixture::new();
    fs::create_dir(f.root.join("elsewhere")).unwrap();
    std::os::unix::fs::symlink(f.root.join("elsewhere"), f.root.join("host")).unwrap();
    assert!(error(&f).contains("symlink"));
}
#[test]
fn rejects_unsafe_branches_and_roots() {
    for branch in [
        "$(id)", "`id`", "--help", "main;id", "a..b", "a.lock", "a//b", "a b", ".hidden", "a/",
        "a.",
    ] {
        let yaml = format!("name: demo\nbackend:\n  repo: github.com/u/r\n  branch: '{branch}'\n");
        assert!(Project::parse(&yaml).is_err(), "{branch}");
    }
    for branch in ["main", "release/v1.2", "feature/my_branch"] {
        assert!(
            Project::parse(&format!(
                "name: demo\nbackend:\n  repo: github.com/u/r\n  branch: '{branch}'\n"
            ))
            .is_ok()
        );
    }
    for root in [
        "/",
        "/tmp/a b",
        "/tmp/../etc",
        "/tmp/a%h",
        "/tmp/a\nUser=root",
    ] {
        assert!(deploy::absolute_root(Path::new(root)).is_err(), "{root}");
    }
    assert!(
        deploy::absolute_root(Path::new("scratch"))
            .unwrap()
            .is_absolute()
    );
}

// Exercise only project-user Git/build actions as the current test user. All
// privileged actions remain unexecuted. Redirect the sample HTTPS URL to a
// local repository for this test only, so no network or credentials are needed.
fn project_actions(steps: &[Step], source: &Path) -> bool {
    for step in steps {
        let Some(action) = &step.command else {
            continue;
        };
        if !matches!(action.identity, Identity::Project(_)) {
            continue;
        }
        let mut cmd = Command::new(&action.program);
        if action.program == "git" {
            cmd.args([
                "-c",
                &format!(
                    "url.file://{}.insteadOf=https://example.invalid/team/app",
                    source.display()
                ),
            ]);
        }
        cmd.args(&action.args);
        if let Some(cwd) = &action.cwd {
            cmd.current_dir(cwd);
        }
        let result = cmd.output().unwrap();
        if !result.status.success() {
            return false;
        }
    }
    true
}
#[test]
fn local_first_deployment_and_redeployment_build_new_revision() {
    let f = Fixture::new();
    let source = f.source();
    let layout = f.layout();
    fs::create_dir_all(layout.project_dir()).unwrap();
    let steps = deploy::inspect(&f.project(), &layout).unwrap();
    assert!(project_actions(&steps, &source));
    let dir = layout.service_dir(sgbc::config::Role::Backend);
    assert_eq!(fs::read_to_string(dir.join("result")).unwrap(), "one\n");
    fs::write(source.join("version"), "two\n").unwrap();
    git(&source, &["commit", "-am", "update"]);
    let steps = deploy::inspect(&f.project(), &layout).unwrap();
    assert!(project_actions(&steps, &source));
    assert_eq!(fs::read_to_string(dir.join("result")).unwrap(), "two\n");
    assert!(steps.iter().any(|s| {
        s.command
            .as_ref()
            .is_some_and(|c| c.program == "systemctl" && c.args[0] == "restart")
    }));
}
#[test]
fn divergent_update_stops_before_build_and_preserves_local_commit() {
    let f = Fixture::new();
    let dir = f.checkout();
    let source = f.root.join("source");
    git(&dir, &["config", "user.name", "Test"]);
    git(&dir, &["config", "user.email", "test@example.invalid"]);
    fs::write(dir.join("version"), "local\n").unwrap();
    git(&dir, &["commit", "-am", "local change"]);
    fs::write(source.join("version"), "remote\n").unwrap();
    git(&source, &["commit", "-am", "remote change"]);
    let steps = deploy::inspect(&f.project(), &f.layout()).unwrap();
    assert!(!project_actions(&steps, &source));
    assert_eq!(fs::read_to_string(dir.join("version")).unwrap(), "local\n");
    assert!(!dir.join("result").exists());
}

#[test]
fn failed_build_stops_local_workflow() {
    let f = Fixture::new();
    let source = f.source();
    fs::write(source.join("build.sh"), "exit 42\n").unwrap();
    git(&source, &["commit", "-am", "failing build"]);
    fs::create_dir_all(f.layout().project_dir()).unwrap();
    let steps = deploy::inspect(&f.project(), &f.layout()).unwrap();
    assert!(!project_actions(&steps, &source));
    assert!(
        !f.layout()
            .service_dir(sgbc::config::Role::Backend)
            .join("result")
            .exists()
    );
}

#[test]
fn services_have_independent_checkout_states_and_builds_never_run_as_root() {
    let f = Fixture::new();
    f.checkout();
    let project = Project::parse("name: demo\nbackend:\n  repo: https://example.invalid/team/app\n  build: sh build.sh\nfrontend:\n  repo: https://example.invalid/team/web\n  build: npm ci && npm run build\n").unwrap();
    let steps = deploy::inspect(&project, &f.layout()).unwrap();
    assert!(
        steps
            .iter()
            .any(|s| s.summary == "Update: fetch backend branch main")
    );
    assert!(
        steps
            .iter()
            .any(|s| s.summary == "First deployment: clone frontend")
    );
    for step in steps {
        if let Some(command) = step.command {
            if command.program == "git" || command.program == "/bin/sh" {
                assert_eq!(command.identity, Identity::Project("demo".into()));
            } else {
                assert_eq!(command.identity, Identity::Root);
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn dry_run_does_not_invoke_repository_fsmonitor_hook() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let dir = f.checkout();
    let hook = dir.join(".git/monitor.sh");
    fs::write(&hook, "#!/bin/sh\ntouch fsmonitor-was-run\n").unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    git(&dir, &["config", "core.fsmonitor", hook.to_str().unwrap()]);
    assert!(f.cli(&["--dry-run"]).status.success());
    assert!(!dir.join("fsmonitor-was-run").exists());
}

#[test]
fn displayed_arguments_round_trip_without_shell_interpretation() {
    use sgbc::plan::PlannedCommand;
    let f = Fixture::new();
    let value = format!("a'b; touch {}/unexpected; $(id)", f.root.display());
    let command = PlannedCommand {
        identity: Identity::Project("demo".into()),
        program: "printf".into(),
        args: vec!["%s".into(), value.clone()],
        cwd: None,
    };
    let display = command.to_string();
    let output = Command::new("/bin/sh")
        .args(["-c", display.strip_prefix("[user=demo] ").unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), value);
    assert!(!f.root.join("unexpected").exists());
}
