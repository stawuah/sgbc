use sgbc::{
    config::{Project, Role},
    deploy,
    layout::Layout,
    render,
};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "sgbc-env-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn parse(extra: &str) -> anyhow::Result<Project> {
    Project::parse(&format!(
        "name: demo\nbackend:\n  repo: github.com/u/r\n{extra}"
    ))
}
fn unit(project: &Project, layout: &Layout) -> String {
    render::systemd_unit(
        project,
        project.backend.as_ref().unwrap(),
        Role::Backend,
        layout,
    )
}

#[test]
fn defaults_and_environment_precedence_are_explicit() {
    let layout = Layout::new("/srv/solanaghana", "demo");
    let old = parse("").unwrap();
    assert!(old.backend.as_ref().unwrap().env.is_empty());
    assert!(!unit(&old, &layout).contains("EnvironmentFile="));
    let project =
        parse("  env:\n    NODE_ENV: staging\n    EMPTY: ''\n  env_file: backend.env\n").unwrap();
    let rendered = unit(&project, &layout);
    assert!(rendered.contains("Environment=\"EMPTY=\""));
    assert!(
        rendered.find("Environment=NODE_ENV=production").unwrap()
            < rendered.find("Environment=\"NODE_ENV=staging\"").unwrap()
    );
    assert!(rendered.contains("EnvironmentFile=/srv/solanaghana/secrets/demo/backend.env"));
}

#[test]
fn rejects_invalid_names_values_and_file_paths_without_echoing_values() {
    for extra in [
        "  env:\n    9KEY: value\n",
        "  env:\n    BAD-KEY: value\n",
        "  env:\n    PORT: '3000'\n",
        "  env:\n    KEY: \"hidden\\nUser=root\"\n",
        "  env:\n    KEY: \"hidden\\0\"\n",
    ] {
        let error = parse(extra).unwrap_err().to_string();
        assert!(!error.contains("hidden"));
    }
    for file in [
        "",
        "../secret",
        "/etc/environment",
        "a/b.env",
        ".env",
        "a%h",
        "a b",
        "*",
        "-a/b",
    ] {
        assert!(parse(&format!("  env_file: '{file}'\n")).is_err(), "{file}");
    }
}

#[test]
fn escapes_systemd_quotes_backslashes_and_specifiers_preserving_dollars() {
    let project = parse("  env:\n    MESSAGE: 'a \"quote\" \\ path %h $HOME = yes'\n").unwrap();
    let rendered = unit(&project, &Layout::new("/srv/solanaghana", "demo"));
    assert!(
        rendered.contains(r#"Environment="MESSAGE=a \"quote\" \\ path %%h $HOME = yes""#),
        "{rendered}"
    );
}

#[cfg(unix)]
#[test]
fn checks_secret_file_metadata_without_reading_contents() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let temp = Temp::new();
    let layout = Layout::new(&temp.0, "demo");
    let project = parse("  env_file: backend.env\n").unwrap();
    let path = layout.env_file("backend.env");
    assert!(deploy::inspect(&project, &layout).is_err());
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(deploy::inspect(&project, &layout).is_err());
    fs::remove_dir(&path).unwrap();
    // Intentionally invalid UTF-8: preflight must never parse secret contents.
    fs::write(&path, [0xff, 0x00]).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(deploy::inspect(&project, &layout).is_err());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(deploy::inspect(&project, &layout).is_ok());
    let actual = temp.0.join("actual");
    fs::rename(&path, &actual).unwrap();
    symlink(&actual, &path).unwrap();
    assert!(deploy::inspect(&project, &layout).is_err());
}

#[cfg(unix)]
#[test]
fn cli_never_copies_or_prints_secret_file_contents() {
    use std::os::unix::fs::PermissionsExt;
    let temp = Temp::new();
    let root = temp.0.join("host");
    let config = temp.0.join("project.yaml");
    fs::write(&config, "name: demo\nbackend:\n  repo: github.com/u/r\n  env:\n    PUBLIC_VALUE: public-marker\n  env_file: backend.env\n").unwrap();
    let path = Layout::new(&root, "demo").env_file("backend.env");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "TOKEN=secret-sentinel-do-not-print\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    for action in ["validate", "plan", "render", "write", "deploy"] {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_sgbc"));
        cmd.arg(action).arg(&config);
        if action != "validate" {
            cmd.arg("--root").arg(&root);
        }
        if action == "deploy" {
            cmd.arg("--dry-run");
        }
        let result = cmd.output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        for output in [&result.stdout, &result.stderr] {
            assert!(!String::from_utf8_lossy(output).contains("secret-sentinel"));
            if action == "deploy" || action == "plan" {
                assert!(!String::from_utf8_lossy(output).contains("public-marker"));
            }
        }
    }
    let service = fs::read_to_string(root.join("systemd/demo-backend.service")).unwrap();
    assert!(!service.contains("secret-sentinel"));
    assert!(service.contains("public-marker"));
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "TOKEN=secret-sentinel-do-not-print\n"
    );
    assert_eq!(
        fs::read_dir(root.join("projects/demo/backend"))
            .unwrap()
            .count(),
        0
    );
}

// Run explicitly where systemd tooling is installed; keep cargo test portable.
#[test]
#[ignore = "requires systemd-analyze; run with cargo test --test env -- --ignored"]
fn generated_environment_unit_passes_systemd_validation_without_warnings() {
    let temp = Temp::new();
    let layout = Layout::new(&temp.0, "demo");
    let project = parse(
        "  env:\n    MESSAGE: 'a \"quote\" \\ path %h $HOME = yes'\n  env_file: backend.env\n",
    )
    .unwrap();
    let path = temp.0.join("demo-backend.service");
    fs::write(&path, unit(&project, &layout)).unwrap();
    let output = Command::new("systemd-analyze")
        .arg("verify")
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
