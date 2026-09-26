# Contributing

Solana Ghana Builder Cloud runs code on a host shared by the whole community, so the bar for a change here is higher than for an ordinary application. This document is about how to meet it.

## Getting set up

You need a Rust toolchain. Nothing else.

```bash
git clone https://github.com/stawuah/sgbc.git
cd sgbc
cargo test
cargo build
```

Then try it against the examples:

```bash
./target/debug/sgbc validate examples/project.yaml
./target/debug/sgbc render   examples/project.yaml
./target/debug/sgbc plan     examples/project.yaml
./target/debug/sgbc write    examples/project.yaml --root ./scratch
```

`--root ./scratch` produces the whole tree as your own user. Use it constantly; you never need root to work on this.

## Verify generated output with the real tools

The point of this project is the files it generates, so check them with the software that will actually consume them rather than by reading them.

```bash
./target/debug/sgbc write examples/project.yaml --root ./scratch

systemd-analyze verify ./scratch/systemd/my-solana-project-backend.service
caddy validate --adapter caddyfile --config ./scratch/configs/my-solana-project.caddy
```

Both should be silent or say `Valid configuration`.

One gotcha: `caddy validate` opens the log files named in the config, so validate against a `--root` you can write to. Validating a config that points at `/srv/solanaghana` fails locally with `permission denied`, which is your filesystem talking, not a syntax error.

## Golden tests

`tests/golden/` holds the exact text generated for each example. A change to a template shows up as a diff there.

```bash
cargo test                      # fails if the output moved
UPDATE_GOLDEN=1 cargo test      # accept the new output
git diff tests/golden/          # read what you changed
```

Always read that diff before committing it. It is the unit a builder's service will run under; `UPDATE_GOLDEN=1` is for recording a change you decided to make, not for making a test go quiet.

## Changing the configuration format

`project.yaml` is a contract with every builder who has already written one.

- Keep `deny_unknown_fields`. A typo that deploys is worse than a typo that fails.
- New fields are optional, with a default that preserves current behaviour.
- Add the field to the reference table in `ARCHITECTURE.md` in the same commit.
- Add an example if the field enables a shape the examples do not yet cover.

## Changing validation

Every rule in `config` exists to stop a specific thing from reaching a systemd unit, a Caddy file or a path. If you relax one, say in the commit message what now becomes possible.

New rules need a test that fails without them. The existing tests name the attack rather than the mechanism — `rejects_a_newline_in_a_command_that_would_inject_unit_directives` — so the reason survives longer than the person who wrote it.

## Changing a systemd unit

The hardening directives are not boilerplate. Before removing one, work out what a builder's process could then reach that it cannot reach today, and put that in the commit message.

If a project genuinely needs write access outside its own directory, widen `ReadWritePaths` for that project through the config. Do not widen it for everyone.

## Execution code

`sgbc` does not yet run deploys, and that is on purpose: those steps run as root against shared infrastructure, and there is no host to test them on.

If you are adding execution, it belongs behind a flag, it needs a dry run that prints exactly what it would do, and it needs to have been run against a real VPS before it is merged. A plausible-looking `Command::new("systemctl")` that nobody has executed is not a contribution to this repository.

## Commits

Explain why, not what. `git log -p` already shows what changed.

Security-relevant changes should say what becomes possible or impossible. Compare:

```
fix: give each project its own log directory

Shared logs/ was chowned to the project user, so any builder could read
and truncate every other builder's logs.
```

against `fix: change log path`. The first one is still useful in a year.
