#!/usr/bin/env python3
"""Exercise the release binary without provisioning a host or contacting repos."""
import hashlib
from pathlib import Path
import subprocess
import tempfile

REPO = Path(__file__).resolve().parents[1]
BINARY = REPO / "target/release/sgbc"
SENTINEL = "sgbc-ci-dummy-secret-never-print"


def run(*args, success=True):
    result = subprocess.run(
        [str(arg) for arg in args], cwd=REPO, capture_output=True, text=True,
        timeout=30,
    )
    output = result.stdout + result.stderr
    if SENTINEL in output:
        raise AssertionError("CLI exposed dummy secret-file contents")
    if (result.returncode == 0) != success:
        raise AssertionError(f"Unexpected exit status for {args[0]}: {result.returncode}\n{output}")
    return result


def snapshot(root):
    """Compare paths, file bytes, and modes; reading may change access times."""
    return {
        str(path.relative_to(root)): (
            path.stat().st_mode,
            hashlib.sha256(path.read_bytes()).hexdigest() if path.is_file() else None,
        )
        for path in root.rglob("*")
    }


def main():
    if not BINARY.is_file():
        raise AssertionError("Build the release binary first: cargo build --release --locked")
    with tempfile.TemporaryDirectory(prefix="sgbc-ci-") as temporary:
        base = Path(temporary)
        examples = sorted((REPO / "examples").glob("*.yaml"))
        if not examples:
            raise AssertionError("No examples to smoke-test")
        for config in examples:
            root = base / config.stem
            run(BINARY, "validate", config)
            run(BINARY, "render", config, "--root", root)
            run(BINARY, "plan", config, "--root", root)
            # Execution must remain unavailable and must never create the root.
            run(BINARY, "deploy", config, "--root", root, success=False)
            if root.exists():
                raise AssertionError("Offline CLI commands wrote deployment files")

            if config.name == "environment.yaml":
                run(BINARY, "deploy", config, "--dry-run", "--root", root, success=False)
                if root.exists():
                    raise AssertionError("Failed preflight wrote deployment files")
                secret = root / "secrets/configured-api/backend.env"
                secret.parent.mkdir(parents=True)
                secret.write_text(f"API_TOKEN={SENTINEL}\n")
                secret.chmod(0o600)

            before = snapshot(base)
            run(BINARY, "deploy", config, "--dry-run", "--root", root)
            if snapshot(base) != before:
                raise AssertionError("Dry run changed files or permissions")

            run(BINARY, "write", config, "--root", root)
            units = sorted((root / "systemd").glob("*.service"))
            if not units:
                raise AssertionError("No systemd units were generated")
            for unit in units:
                if SENTINEL in unit.read_text():
                    raise AssertionError("Generated unit contains secret-file contents")
                verified = run("systemd-analyze", "verify", unit)
                # systemd can return success while ignoring an invalid directive.
                if verified.stderr.strip():
                    raise AssertionError(f"systemd verification warning: {verified.stderr}")
            before = snapshot(base)
            run(BINARY, "deploy", config, "--dry-run", "--root", root)
            if snapshot(base) != before:
                raise AssertionError("Dry run changed an existing generated tree")
            print(f"PASS: {config.name} release CLI and generated units")


if __name__ == "__main__":
    main()
