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
cargo build --release

sgbc validate examples/project.yaml   # check the configuration
sgbc render   examples/project.yaml   # print the systemd units and Caddy config
sgbc plan     examples/project.yaml   # print the ordered deploy steps
sgbc write    examples/project.yaml --root ./scratch   # write the files somewhere safe
```

`--root` exists so the whole tree can be produced and inspected without a host. It defaults to `/srv/solanaghana`.

Deliberately absent: `sgbc` does not create Linux accounts, clone repositories, run builds, install units into `/etc`, or start services. Those are the steps `sgbc plan` prints, and they belong to Phase 2 — when there is a host to run them against and test them on. Shipping privileged, untested execution ahead of that would be the one mistake this platform cannot afford.

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

---

Built for the Solana Ghana builder community. 🇬🇭☀️
