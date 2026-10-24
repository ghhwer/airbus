# AGENTS.md — contributing to this repository

You are editing the **Airbus monorepo** as a contributor (human or agent).
This is not the adopter brief.

| Audience | Start here |
| -------- | ---------- |
| Patch / extend this git tree | **This file** → [`CONTRIBUTING.md`](CONTRIBUTING.md) |
| Help someone *run or integrate* Airbus elsewhere | [`docs/agents/`](docs/agents/README.md) |

Claude Code and similar tools: see [`CLAUDE.md`](CLAUDE.md) (points here).

---

## Docs boundary (do not mix these)

| Path | Who it is for | What it is |
| ---- | ------------- | ---------- |
| **`AGENTS.md`** (here) | Agents spawned **on this repo** | Contributor orientation + traps |
| **`CONTRIBUTING.md`** | Anyone changing the tree | Setup, codegen, publish, env |
| **`docs/agents/`** | Agents helping an **adopter** | Positioning, consume API, hard limits — *not* how to patch `src/` |
| **`docs/clients/`** | Integrators | How to call the daemon from Python / Rust / ESP32 |
| **`docs/architecture.md`**, **`protocol.md`**, **`queues-and-dispatch.md`**, **`overview.md`** | Shared technical truth | Daemon layers, wire format, queue semantics |
| **`README.md`** | First contact | Product pitch + Docker quickstart |

**Rule:** If the task is “change Airbus itself,” prefer CONTRIBUTING + architecture/protocol over `docs/agents/CONTEXT.md`. That CONTEXT file is written for consumers; its “If editing this repository” footnote only redirects you back here.

---

## What this project is (contributor view)

- One Rust daemon (`src/`): JSON-RPC 2.0 over TCP (`:9097`), optional HTTP UI (`:9098`).
- Contracts live in **`schema/`** (OpenRPC + JSON Schema). Typed payloads are **generated**, not authored by hand.
- Language clients are separate trees: `client-py/`, `client-rs/`, `client-embedded/`.
- State is **in-memory**; do not design or document durability/clustering that the code does not provide.

---

## Working rules

1. **Schema first** — extend `schema/openrpc.json` and `schema/payloads/`, then `make generate`. Commit generated outputs.
2. **Never hand-edit** generated payloads (`src/proto/payloads.rs`, `client-rs/src/payloads.rs`, `client-py/.../payloads.py`, `client-embedded/.../payloads.h`).
3. **Do not invent RPC methods** in clients or docs without schema + codegen.
4. **Respect layers** in `src/`: `app` must not touch wire envelopes; stay inside existing `app` / `io` / `proto` / `runtime` boundaries (`make check-protocol` / `make test`).
5. **Verify** with `make test` (and `make generate` when contracts change). Local loop: `make setup && make build && make run`.

Details and publish paths: [`CONTRIBUTING.md`](CONTRIBUTING.md).

---

## Confusions to avoid

| Trap | Reality |
| ---- | ------- |
| Reading `docs/agents/` as the contributor playbook | That folder is for **adopters**. Use this file + CONTRIBUTING. |
| Treating client READMEs as daemon internals | Client docs describe **calling** the service; daemon design is `docs/architecture.md` + `src/`. |
| Editing `payloads.*` to “fix a type” | Fix the **schema**, regenerate. |
| Assuming Kafka / Rabbit / Redis semantics | Airbus is a small in-memory bus; queue modes are `broadcast` / `fifo` (+ strategies) / `full-duplex` — see `docs/queues-and-dispatch.md`. |
| Mixing HTTP `/rpc` with the primary TCP path | Primary client path is TCP JSON-RPC on `:9097`; HTTP is UI + alternate RPC. |
| Confusing `docs/overview.md` product framing with implementation TODOs | Overview is positioning; implementation truth is schema + `src/` + architecture/protocol docs. |
| Pointing end users at CONTRIBUTING or AGENTS.md | Adopters get README → `docs/clients/` or `docs/agents/`. |

---

## Quick map

```text
schema/          ← source of truth for RPC + payloads
scripts/         ← codegen, protocol checks, packing
src/             ← daemon
client-*/        ← published clients
resources/ui/    ← debug UI assets
tests/           ← daemon / protocol tests
docs/agents/     ← AI adopters only (not you, if editing)
```

Deeper: [Architecture](docs/architecture.md) · [Protocol](docs/protocol.md) · [Queues](docs/queues-and-dispatch.md).
