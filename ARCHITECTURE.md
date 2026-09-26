# Architecture

How `sgbc` turns one YAML file into the systemd units, Caddy routes and directory layout a project needs on a shared host.

## Shape of the tool

`sgbc` is a single Rust binary with no runtime dependencies, which is what makes it a clean systemd citizen: nothing to install, nothing to keep at a version, nothing to leak between projects.

| Module | Responsibility |
| --- | --- |
| `config` | Parse `project.yaml` and enforce every constraint the host relies on |
| `layout` | Derive every path from the project name |
| `render` | Generate the systemd unit and the Caddy site block |
| `plan` | Produce the ordered steps a deploy performs |

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

domain:                        # optional
  frontend: myproject.solanaghana.dev
  api: api.myproject.solanaghana.dev
```

At least one of `frontend` or `backend` is required. Unknown fields are rejected, so a typo like `prot: 3000` fails loudly instead of silently deploying without a port.

### Why only two service slots

`frontend` is the browser-facing service. `backend` is the generic slot: an API, a worker, a bot, an indexer and an MCP server all deploy through it. A service with no `port` and no domain is never routed, which is the correct shape for a worker — `examples/worker.yaml` shows it.

The honest limitation: a project needing two backends cannot express that yet. Generalising to a named `services:` map is Phase 2 work, and it is a config format change, so it is worth doing once the first real projects have shown what they actually need.

## Validation is the trust boundary

Every value in the config ends up in a systemd unit, a Caddy site block or a filesystem path. `config::validate` is therefore stricter than YAML requires, and each rule exists for a specific reason:

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
