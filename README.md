# Solana Ghana Builder Cloud

A lightweight deployment platform for builders in the Solana Ghana community.

Solana Ghana Builder Cloud provides shared infrastructure for builders to deploy, host, and ship Solana projects without managing their own VPS.

The goal is simple:

**Build → Push to GitHub → Deploy → Ship. 🇬🇭**

## What it provides

Builders can deploy applications such as:

- Frontends
- Node.js / TypeScript backends
- Rust services
- APIs
- MCP servers
- Bots and workers
- Indexers and supporting services

Smart contracts remain deployed directly to Solana. The Builder Cloud is primarily for the off-chain infrastructure around those projects.

## Architecture

```
                    GitHub
                       │
                       ▼
              Deployment System
                       │
                 ┌─────┴─────┐
                 │            │
              Build        Configure
                 │            │
                 └─────┬──────┘
                       ▼
                    systemd
                       │
                 Builder App
                       │
                       ▼
                     Caddy
                       │
                       ▼
              Cloudflare / DNS
                       │
                       ▼
          project.solanaghana.dev
```

### Core infrastructure

| Component | Purpose |
| --- | --- |
| Ubuntu | Server operating system |
| systemd | Runs and supervises applications |
| Caddy | Reverse proxy and HTTPS |
| Cloudflare | DNS and domain management |
| GitHub | Source code |
| Deployment service | Builds and deploys projects |
| YAML | Project configuration |

No Docker is required.

## How deployment works

A project can define its deployment configuration with a YAML file:

```yaml
name: my-solana-project

frontend:
  repo: github.com/user/frontend
  build: npm run build
  port: 3000

backend:
  repo: github.com/user/backend
  build: npm run build
  port: 8080

domain:
  frontend: myproject.solanaghana.dev
  api: api.myproject.solanaghana.dev
```

The deployment system handles:

- Pulling the repository
- Installing dependencies
- Building the application
- Creating the required systemd service
- Starting/restarting the application
- Configuring the Caddy route
- Making the project available through its subdomain

## Builder experience

The long-term goal is:

```
Create account
      ↓
Connect GitHub
      ↓
Select repository
      ↓
Deploy
      ↓
project.solanaghana.dev
```

Builders should not need:

- VPS credentials
- Root access
- SSH access
- Docker
- Caddy configuration
- systemd knowledge
- Manual server configuration

## V1

The first version will intentionally stay simple.

### V1 flow

```
GitHub repository
       ↓
Deployment script
       ↓
Ubuntu VPS
       ↓
systemd
       ↓
Caddy
       ↓
Public subdomain
```

The first deployment can be triggered manually while the infrastructure is being tested.

GitHub webhooks/API integration and a builder dashboard can be added later.

## The `sgbc` tool

`sgbc` reads a project's configuration and generates everything a deploy installs. Phase 1 generates and reviews; it does not execute.

```bash
cargo install --path .    # puts sgbc on your PATH

sgbc validate examples/project.yaml   # check the configuration
sgbc render   examples/project.yaml   # print the systemd units and Caddy config
sgbc plan     examples/project.yaml   # print an offline first-deploy template
sgbc deploy   examples/project.yaml --dry-run --root ./scratch  # inspect local checkouts
sgbc write    examples/project.yaml --root ./scratch   # write the files somewhere safe
```

Or without installing, running the binary from the build directory:

```bash
cargo build --release
./target/release/sgbc validate examples/project.yaml
```

`--root` exists so the whole tree can be produced and inspected without a host. It defaults to `/srv/solanaghana`.

Deliberately absent: `sgbc` does not create Linux accounts, clone repositories, run builds, install units into `/etc`, or start services. Those are the steps `sgbc plan` prints, and they belong to Phase 2 — when there is a host to run them against and test them on. Shipping privileged, untested execution ahead of that would be the one mistake this platform cannot afford.

### Deployment dry run

`sgbc deploy project.yaml --dry-run --root ./scratch` inspects service checkout
paths under the selected **local** root and prints a deployment plan. It does
not contact GitHub or a VPS, fetch code, run builds, write deployment files, or
invoke systemd/Caddy. Omitting `--dry-run` is an error: execution is unavailable.

- Missing or empty service directories produce a first-deployment clone plan.
- Existing clean checkouts on the configured branch and origin produce a fetch
  and fast-forward update plan, followed by a service restart.
- Dirty checkouts, untracked files, different origins/branches, detached HEADs,
  non-repository directories, and symlinked checkout paths are rejected.
- Every command identifies its intended root or project-user identity. These
  labels are explanatory, not a shell script to paste into a root terminal.
- Relative roots resolve to absolute paths. Roots accept letters, digits,
  slashes, hyphens, underscores and dots; `/` and parent traversal are rejected.
  This path rule also applies to `render`, `plan`, and `write` because their
  paths appear in systemd and Caddy files.

The plan requires operator provisioning for accounts, runtimes, Caddy imports,
DNS and log permissions. Include dependency installation in `build`, for example
`npm ci && npm run build`. Updates are in place, have no rollback or health
checks, and must stop on failure. Remote branch existence and divergence can
only be checked when a future executor fetches; this dry run does not do so.
`plan` remains an offline template that assumes new checkouts.

Local tests exercise the planned Git/build actions against a temporary repository
as the current test user, including first deployment, updates and failure cases.
They do not establish host permissions, service startup or HTTPS behavior.

See [ARCHITECTURE.md](ARCHITECTURE.md) for the configuration reference and the generated output, and [CONTRIBUTING.md](CONTRIBUTING.md) to work on it.

## Security model

The VPS is shared infrastructure.

Builders should not receive root access.

Each project should have its own:

- Project directory
- Linux user/process permissions
- systemd service
- Port
- Domain/subdomain

For the initial version, the platform is intended for trusted Solana Ghana builders, rather than arbitrary public code execution.

Future versions can introduce stronger isolation and resource controls.

## Project structure

A possible server structure:

```
/srv/solanaghana/
│
├── projects/
│   ├── project-one/
│   ├── project-two/
│   └── project-three/
│
├── deployments/
│
├── configs/
│
└── logs/
```

## Development roadmap

### Phase 1 — Local development

- [x] Deployment configuration format
- [x] systemd service templates
- [x] Caddy configuration generator
- [x] Project directory structure
- [x] Basic security model
- [x] Local testing
- [ ] Deployment script (execution; deliberately Phase 2, see above)

### Phase 2 — VPS

- [ ] Provision VPS
- [ ] Configure Ubuntu
- [ ] Harden SSH
- [ ] Install Caddy
- [ ] Configure systemd
- [ ] Configure Cloudflare DNS
- [ ] Deploy first project
- [ ] Test HTTPS/subdomains

### Phase 3 — GitHub integration

- [ ] GitHub authentication
- [ ] Repository selection
- [ ] Deployment triggers
- [ ] Deployment logs
- [ ] Automatic redeployment

### Phase 4 — Builder dashboard

- [ ] Builder accounts
- [ ] Project creation
- [ ] GitHub connection
- [ ] Deploy button
- [ ] Project status
- [ ] Logs
- [ ] Domain management

## Vision

Solana Ghana Builder Cloud is intended to make infrastructure less of a barrier for Ghanaian builders.

Instead of every builder needing to figure out:

> VPS + Linux + SSH + reverse proxy + SSL + deployment

the community can provide the infrastructure and let builders focus on:

**Build → Ship → Iterate.**

## License

Apache-2.0. See [LICENSE](LICENSE).

Copyright 2026 Solana Ghana Community.

---

Built for the Solana Ghana builder community. 🇬🇭☀️
