# AgentMesh contributor guide (Phase 0)

## Scope

Private workspace. Phase 0 host/plugin contract is Accepted. Phase 1.0 may add Multica **adapter plugins** with opaque payloads only.

Do not: change host envelope ownership, promote Multica/`WorkItem` types into `agentmesh-proto`/`agentmesh-host`, add daemon/TUI, claim production cutover, or claim polyglot proof.

Phase 1.0+ **local production control foundation** (ADR 0003) adds bounded plugin-owned components only: pinned Multica CLI adapter, app-local SQLite control ledger, observer one-shot wiring, and Task Scheduler script generation. These stay outside core/protocol crates; live Multica mutation beyond fakes/synthetics remains deferred until later authority slices.

## Workspace rules

- Only the coordinator lane edits root `Cargo.toml` / `Cargo.lock`.
- Python port crates (`docs/python-migration-v0.md`) are the one exception: a crate under `plugins/ops/<name>/` joins the workspace through the `plugins/ops/*` member glob, so adding it needs no root `Cargo.toml` edit. Such a PR may change `Cargo.lock` only by its own package entries. Resolve a `Cargo.lock` conflict by rebasing on `main` and regenerating with `cargo check --workspace`, never by hand. Everything outside `plugins/ops/*` stays coordinator-only.
- `plugins/ops/` holds crate directories only (a non-crate entry breaks the glob). `plugins/ops/kit` (`agentmesh-ops-kit`) is coordinator-owned; port PRs depend on it by path and do not change it.
- Ops crates must not depend on `agentmesh-host` / `agentmesh-conformance`, and do not call Multica, git or the network themselves; side effects stay behind a trait or in the vault wrapper.
- Python stays the authority for every ported controller until the owner switches it. A port PR never changes which implementation the vault runs.
- Every crate is `publish = false`.
- Fixture plugins depend on `agentmesh-fixture-support` + `agentmesh-proto` only — never `agentmesh-host` or `agentmesh-conformance`.
- Phase 1 adapter plugins (`agentmesh-multica-*`, `agentmesh-markdown-*`) must not depend on `agentmesh-host` / `agentmesh-conformance`; adapter-shaped types stay inside the plugin crate.
- Malformed framing/JSON fixtures use independent raw writers.
- No vault paths, Multica types, or secrets in core/fixtures; plugin testdata may use synthetic recorded Multica-shaped JSON only.
- Local production control plugins (`agentmesh-multica-cli-adapter`, `agentmesh-local-control-ledger`, `agentmesh-production-controller-observer`, `agentmesh-production-authority`, `agentmesh-production-evaluation-report`) must not depend on `agentmesh-host` / `agentmesh-conformance`; Multica CLI paths and SQLite schema stay plugin-owned.

## Agent documentation discovery

Prefer the installed binary's offline catalog before searching repository files:

```bash
agentmesh docs list
agentmesh docs show <exact-name>
```

`docs show` accepts catalog names only and returns embedded Markdown in compact JSON. Do not
pass paths, infer filenames, or depend on the current working directory.

## Checks before PR

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Documentation status

- `docs/protocol-v0.md` / `docs/threat-model-v0.md` — Accepted for Phase 0
- ADR 0001 — Accepted
- ADR 0003 — Accepted (local production control foundation)
- `docs/phase0-exit-review.md` — PASS; Phase 1.0 slices include shadow Multica selector skeleton and Markdown request validator App
