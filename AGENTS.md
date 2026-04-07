# AGENTS.md — git_agent_race_controller

This file governs the entire `git_agent_race_controller/` repository.

## Mission
Build `garc` (Git Agent Race Controller), a production-quality Rust CLI that prevents branch-level Git collisions between autonomous coding agents sharing a LAN.

The source of truth is `docs/spec.md`.
If implementation details are unclear, prefer the spec over assumptions.

## Product contract
- Binary name: `garc`
- Primary commands:
  - `garc init`
  - `garc checkout <branch>`
  - `garc status`
- Main behavior:
  - Detect whether the requested branch is already occupied by another CAMP peer in the same project.
  - If clear, checkout the requested branch.
  - If occupied, create and checkout a diverted sub-branch named `<requested>--<local_agent_id>`.
  - `--force` bypasses collision checks.
  - `--json` must emit predictable machine-readable output.
- `garc init` must install an idempotent `post-checkout` hook that keeps CAMP mesh state synchronized even when users bypass `garc` and call `git` directly.

## Required technical choices
Use these unless a stronger repo-local reason appears during implementation:
- CLI parsing: `clap` v4
- Git operations: `git2`
- Serialization: `serde`, `serde_json`
- mDNS/CAMP discovery: prefer `mdns-sd`
- Error handling: `anyhow` for app flow and/or `thiserror` for domain errors

## Expected module layout
Keep code small, typed, and modular. Prefer this structure unless a better split emerges naturally:
- `src/main.rs` — entrypoint and top-level error reporting
- `src/cli.rs` — clap command/flag definitions
- `src/engine.rs` — collision detection and branch diversion policy
- `src/installer.rs` — Git hook installation/update logic
- `src/git.rs` — repository discovery and checkout helpers
- `src/mesh.rs` — CAMP/mDNS discovery + local broadcast helpers
- `src/output.rs` — human/json response shapes
- `src/errors.rs` — domain error types, if needed

Do not create layers with no clear payoff.
Prefer deletion and consolidation over speculative abstraction.

## CAMP compatibility rules
Treat the CAMP network as an mDNS-discoverable mesh using a service type compatible with the spec, e.g. `_camp._tcp.local`.
TXT records should carry at least:
- `agent_id`
- `current_branch`
- `current_project`

Implementation guidance:
- Compare mesh peers only within the same `current_project` as the current repo.
- Rely on CAMP/mDNS TTL eviction for orphaned locks; do not invent a separate lock server.
- Add inline comments around TTL-related behavior because stale peer disappearance is a core correctness assumption.

## Git behavior rules
- Use `git2` for production Git operations.
- Avoid shelling out to `git` in normal command execution unless there is no robust `git2` equivalent.
- The generated `post-checkout` hook may call `camp update --branch ...` as specified.
- Hook installation must be idempotent:
  - running `garc init` twice must not duplicate injected content
  - preserve unrelated existing hook content
  - prefer marker comments around managed sections

## Output contract
When `--json` is passed, stdout must stay stable and easy for LLMs/programs to parse.
Model outputs with typed structs and serialize them directly.
Support at least the states described by the spec:
- clear/successful checkout
- diverted checkout due to occupation
- forced checkout
- errors should be structured if JSON mode is active

## Code quality rules
- No `unwrap()` / `expect()` in production paths.
- Propagate context-rich errors.
- Keep branch-collision decisions expressed as typed enums, e.g. `CollisionResult::{Clear, Occupied { by }}`.
- Add inline comments only for non-obvious design decisions.
  Required comment areas:
  - TTL/orphaned-lock behavior
  - sub-branch naming convention
  - hook idempotence markers
- Keep functions short and side effects explicit.
- Reuse standard library and existing modules before adding helpers.
- No new dependencies beyond the spec unless clearly justified.

## Commit and agent-knowledge rules
- Treat git history as part of the agent memory for this repo.
- Every meaningful change should be committed with a Conventional Commit style subject:
  - `feat: ...`
  - `fix: ...`
  - `refactor: ...`
  - `test: ...`
  - `docs: ...`
  - `ci: ...`
  - `chore: ...`
- Prefer an optional scope when it improves clarity, e.g. `feat(mesh): ...` or `fix(cli): ...`.
- The first line should say **why** the change exists, not just what files changed.
- For non-trivial commits, include brief lore-style trailers so future agents can recover intent quickly:
  - `Constraint: ...`
  - `Rejected: ...`
  - `Confidence: low|medium|high`
  - `Scope-risk: narrow|moderate|broad`
  - `Directive: ...`
  - `Tested: ...`
  - `Not-tested: ...`
- Do not batch unrelated changes into one commit; preserve a clean, searchable knowledge trail for later agents.

## Testing and verification
Before claiming work complete, run the smallest relevant full set:
- `cargo fmt --all`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test`

Also verify behavior relevant to the changed area, for example:
- JSON output snapshots / assertions
- branch diversion naming
- hook installer idempotence
- mesh collision filtering by project

Prefer unit tests for engine/output logic and temp-repo integration tests for Git/hook behavior.

## Scope discipline
This repo is for `garc` only.
Do not add unrelated framework code, daemon processes, or central coordination services.
If CAMP integration details are missing, implement the thinnest compatible mDNS layer that satisfies the spec and document assumptions in code comments or README-level notes.

## Completion checklist
A task is not done until all of the following are true:
- code compiles
- relevant tests pass
- fmt + clippy pass cleanly
- JSON output paths are verified
- hook installation is idempotent
- final summary lists changed files, key decisions, and remaining risks
