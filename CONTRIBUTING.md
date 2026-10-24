# Contributing to Airbus

This file is for people (and agents) **changing this repository**. If you only
want to run the daemon and call the API, use the [README](README.md) and
[`docs/clients/`](docs/clients/).

## AI-assisted contributions

We don't mind AI at all. Use it if it helps.

Don't be an idiot about it: read the docs, follow schema-first / codegen rules,
and only contribute when you understand what the change does and why. Verify
(`make test`, and `make generate` when contracts change). If you don't
understand the diff — or you don't care — don't touch the tree. This repo
should be edited by people who give a shit.

Not wanting to touch the code is fine — open an issue instead; someone will
look. AI-assisted issues are OK too, as long as they are not an essay: short
repro / ask, what you expected, what happened.

## Develop locally

```bash
make setup && make build && make run
# TCP 0.0.0.0:9097 — UI http://127.0.0.1:9098/
```

```bash
make test       # protocol checks + unit + client integration
make generate   # regenerate typed payloads from schema/
make help       # all targets
```

Local image (instead of GHCR): `docker compose up --build` or
`make docker && make docker-run`.

CLI shape:

```text
airbus --listen [host:]port [--http [host:]port --resources DIR]
```

## Repo map

| Path | Role |
| ---- | ---- |
| `schema/` | Payload JSON Schema + OpenRPC — **source of truth** |
| `scripts/` | Codegen + protocol boundary checks + release packing |
| `src/` | Rust daemon (`app` / `io` / `proto` / `runtime`) |
| `client-py/` / `client-rs/` / `client-embedded/` | Language clients |
| `resources/ui/` | Debug UI static assets |
| `docs/` | Human docs; `docs/agents/` is for AI adopters |
| `tests/` | Daemon / protocol tests |

## Schema / codegen

Edit contracts under `schema/`, then:

```bash
make generate
# typify → src/proto/payloads.rs + client-rs/src/payloads.rs
# datamodel-codegen → client-py/.../payloads.py
# scripts → client-embedded/.../payloads.h
```

Requires `cargo-typify` (`cargo install cargo-typify --version 0.8.0`) and
`make setup`. Generated sources are committed — do not hand-edit them.

`make check-protocol` / `make test` enforce boundaries (app layer must not
touch envelopes; payloads stay generated).

## Publishing (maintainers)

After tagging `v*`:

| Artifact | How |
| -------- | --- |
| Container `ghcr.io/ghhwer/airbus` | `.github/workflows/publish-image.yml` |
| PyPI `airbus-client` | `make publish-client` or publish-client workflow |
| crates.io `airbus-client` | `make publish-client-rs` or publish-client-rs workflow |
| ESP32 PlatformIO zip | `make pack-client-embedded` / publish-client-embedded workflow |

## Environment (dev / tests)

| Variable | Default | Purpose |
| -------- | ------- | ------- |
| `AIRBUS_URL` | `127.0.0.1:9097` | Client TCP endpoint |
| `AIRBUS_HOST` / `AIRBUS_PORT` | `127.0.0.1` / `9097` | Client fallbacks |
| `AIRBUS_BIN` | `out/airbus` | Binary for tests / `start_server` |
| `AIRBUS_HTTP_URL` | `0.0.0.0:9098` | HTTP listener (`make run`) |
| `AIRBUS_RESOURCES` | `resources/ui` | Static UI assets |

## Deeper reading

- [Architecture](docs/architecture.md) — daemon layering
- [Protocol](docs/protocol.md) — wire + schema pipeline
- [Queues & dispatch](docs/queues-and-dispatch.md) — delivery semantics
- [AGENTS.md](AGENTS.md) — orientation for agents spawned on this repo
  (docs boundary vs `docs/agents/`, traps to avoid)

### Steering an agent that is *contributing*

```text
You are editing the Airbus repository. Read AGENTS.md and CONTRIBUTING.md.
Follow schema-first codegen (make generate). Do not hand-edit generated
payloads. Do not invent RPC methods — extend schema/openrpc.json.
```

Agents helping someone *adopt* Airbus (not patch this tree) should use
[`docs/agents/`](docs/agents/README.md) instead.
