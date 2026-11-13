# Architecture

How `sgbc` turns one YAML file into the systemd units, Caddy routes and directory layout a project needs on a shared host.

## Shape of the tool

`sgbc` is a single Rust binary with no runtime dependencies, which is what makes it a clean systemd citizen: nothing to install, nothing to keep at a version, nothing to leak between projects.

| Module | Responsibility |
| --- | --- |
| `config` | Parse `project.yaml` and enforce every constraint the host relies on |
| `layout` | Derive every path from the project name |
| `render` | Generate the systemd unit and the Caddy site block |
| `plan` | Produce structured commands with arguments, working directories and execution identities |
| `deploy` | Read-only local checkout inspection for `deploy --dry-run` |

`render` and `plan` are pure functions of the config and the layout. Nothing in them touches the filesystem, which is why the exact text destined for a host can be tested without one — see `tests/golden.rs`.

## Configuration reference

```yaml
name: my-solana-project        # required

frontend:                      # optional
  repo: github.com/user/app    # required
  branch: main                 # optional, defaults to main
  build: npm run build         # optional
  start: npm run start         # optional, defaults to `npm run start`
  port: 3000                   # required if a domain routes here

backend:                       # optional
  repo: github.com/user/api
  build: cargo build --release
  start: ./target/release/api
  port: 8080
  env:                         # optional public runtime strings
    LOG_LEVEL: info
  env_file: backend.env        # optional filename under <root>/secrets/<name>/

domain:                        # optional
  frontend: myproject.solanaghana.dev
  api: api.myproject.solanaghana.dev
```

At least one of `frontend` or `backend` is required. Unknown fields are rejected, so a typo like `prot: 3000` fails loudly instead of silently deploying without a port.

### Why only two service slots

`frontend` is the browser-facing service. `backend` is the generic slot: an API, a worker, a bot, an indexer and an MCP server all deploy through it. A service with no `port` and no domain is never routed, which is the correct shape for a worker — `examples/worker.yaml` shows it.

The honest limitation: a project needing two backends cannot express that yet. Generalising to a named `services:` map is Phase 2 work, and it is a config format change, so it is worth doing once the first real projects have shown what they actually need.

## Validation is the trust boundary

Every value in the config ends up in a systemd unit, a Caddy site block or a filesystem path. The validation in `Project::validate` and `Service::validate` is therefore stricter than YAML requires, and each rule exists for a specific reason:

| Rule | What it prevents |
| --- | --- |
| `name` is `[a-z][a-z0-9-]*`, max 40 | The name becomes a directory, a unit name and a Linux username. Anything else could escape the project root or forge a unit name. |
| Commands must be a single line | A newline in `build` would let a project append arbitrary directives — `ExecStartPre=`, `User=root` — to its own generated unit. |
| `repo` rejects shell metacharacters | The repo is handed to `git`. `;`, `` ` ``, `$`, quotes and whitespace are refused. |
| `port` must be ≥ 1024 | Privileged ports need capabilities no builder process is granted. |
| Two services cannot share a port | The second service would fail to bind, after the first had already been declared healthy. |
| A routed service must declare a port | Caddy cannot proxy to a service with no address. |
| Hostnames are checked label by label | A malformed name would be accepted here and then rejected by Caddy at reload, taking every other project's route down with it. |

The property these rules protect is asserted directly: `tests/golden.rs` walks every generated path and requires it to stay under the configured root with no `..` component.

## Generated systemd unit

One unit per service, named `<project>-<role>.service`.

The command runs through `/bin/sh -lc` because systemd requires an absolute `ExecStart` path while builders write ecosystem commands like `npm run start`. A literal single quote in a command is escaped the POSIX way rather than rejected, and the escaping is verified by round-tripping through a real `/bin/sh` in `render`'s tests.

Each unit is hardened, because the host runs code its operators did not write:

| Directive | Effect |
| --- | --- |
| `User` / `Group` | One Linux account per project |
| `ProtectSystem=strict` | The entire filesystem is read-only except what is granted back |
| `ReadWritePaths` | Only this project's own directory and its own log directory |
| `NoNewPrivileges` | No path to privilege escalation |
| `PrivateTmp`, `PrivateDevices` | No shared `/tmp`, no device access |
| `ProtectHome` | No access to any user's home directory |
| `MemoryMax=512M`, `TasksMax=256` | One project cannot exhaust the host |

## Generated Caddy configuration

One fragment per project at `configs/<project>.caddy`, holding one site block per service that has both a domain and a port. Caddy provisions HTTPS for each name on its own, so nothing here handles certificates.

A project with no domain gets a fragment containing only a comment. That is the expected output for a worker, not an error.

## Filesystem layout and isolation

```
/srv/solanaghana/
├── projects/            root:root 755   — platform owned
│   └── <project>/       <project> 750   — the project's checkouts
├── deployments/         root:root 755   — platform owned
│   └── <project>/       <project> 750   — deploy records
├── configs/             root:root 755   — generated Caddy fragments
├── logs/                root:root 755   — platform owned
│   └── <project>/       <project> 750   — this project's logs only
└── systemd/             root:root 755   — generated units, before install
```

The split matters. Shared directories stay root-owned; only a project's own subtree is handed to its service account. An earlier draft chowned the shared `logs/` and `configs/` directories to the project user, which would have let any builder read and truncate every other builder's logs — the exact isolation the security model rests on. Per-project log directories are the fix.

Generated units are written to `<root>/systemd/` rather than straight into `/etc/systemd/system/`. A deploy can then produce and review a complete tree as an unprivileged user, and only the install step needs root.

## What Phase 1 does not do

`sgbc` generates files and prints the plan. It does not create accounts, clone repositories, run builds, install into `/etc`, or start services.

That boundary is deliberate. Those steps run as root on a host shared by the community, and there is no host yet to test them against. Untested privileged code is the one thing this platform cannot afford to ship early, so Phase 2 begins by writing that execution path against a real VPS where each step can be proven.

## Local deployment preflight

`deploy --dry-run` resolves the selected root to an absolute path and inspects
backend then frontend checkouts. Empty paths are new deployments. Existing
checkouts must be ordinary Git repositories with the expected origin and branch,
no tracked changes and no untracked files. Symlinked ancestors and Git directories
are rejected. Worktrees and detached HEADs are not supported. Git metadata reads
use `--no-optional-locks` and disable filesystem monitor hooks and untracked cache;
no fetch, build or planned privileged command is executed.

`PlannedCommand` stores program, argument vector, working directory and identity
separately. Its shell-quoted display is for review; it is not an executor.
Git and build actions have a project-user identity. Directory provisioning, unit
installation and service control have a root identity. Account verification and
fresh validated configuration staging remain explicit prerequisites, not implicit
claims that the host is ready.

An update plans `git fetch` followed by `git merge --ff-only FETCH_HEAD`. This
preserves local history and fails on divergence; it does not reset or delete local
commits. A locally ahead branch can remain ahead, so this is not an exact remote
revision deployment. Both new and existing services plan `enable` then `restart`.
All builds precede service installation, but builds still modify the checkout in
place. Atomic releases, rollback, build sandboxing/resource limits, revision
pinning, application health checks and host-wide port/domain allocation remain
future execution work. Preflight is a point-in-time local check and must be
repeated by a future executor before mutation.

The branch validator accepts a conservative subset of Git ref names: letters,
digits, hyphens, underscores, dots and slashes, with no leading option marker,
empty/dot-prefixed components, `.lock` suffixes, trailing dots or `..` sequences.
Root paths also use a restricted character set because renderers insert them
into systemd/Caddy syntax. Neither restriction turns arbitrary builder commands
into trusted code; they must never be executed as root.

## Runtime environment configuration

Each service accepts an optional `env` map and `env_file` filename. Both are
runtime-only: build commands do not receive them. Existing configurations retain
their generated output. `env` keys use `[A-Za-z_][A-Za-z0-9_]*`; values must not
contain control characters. Quote YAML values to make their intended string form
clear. `PORT` is reserved in `env`; use the service's `port` field instead.

Public values are rendered deterministically into quoted `Environment=` entries.
Backslashes and double quotes are escaped, and literal percent signs are doubled
to prevent systemd specifier expansion. Dollar signs remain literal. Explicit
`env.NODE_ENV` overrides the default `production`. Public values appear in rendered
and written units, but plans omit their values. Never place secrets in `env`.

`env_file: backend.env` references `<root>/secrets/<project>/backend.env` through a
required `EnvironmentFile=` directive. Only a filename is accepted (letters,
digits, dots, hyphens and underscores, no leading dot); arbitrary paths, globs,
specifiers and traversal are rejected. `write` does not create secret directories
or copy secret files. The operator provisions them separately, outside checkouts,
with root ownership, protected parent directories and file mode 600.

`validate`, `render`, `plan` and `write` need no local secret file. `deploy --dry-run`
requires an existing regular file, rejects symlinked paths and, on Unix, requires
owner-read permission with no group/other permissions. It inspects metadata only;
contents, syntax, ownership and parent-directory access policy are not validated.
The system service manager reads the file when starting the app. File values take
precedence over inline settings, including PORT, so operators must keep PORT
consistent with routing. Use systemd environment-file syntax (`KEY=value`, no
`export`); this is not a shell script and no shell substitutions are performed.

This keeps values out of generated unit files and sgbc output; it is not an
encrypted secret store. The application receives an ordinary process environment.
See [systemd.exec](https://www.freedesktop.org/software/systemd/man/latest/systemd.exec.html#Environment=)
for the environment and environment-file semantics.
