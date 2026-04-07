# GARC — RAI Alignment Specification

> Gap analysis between `garc` v0.1.1 and the RAI architecture in `PROJECTS.md`.
> Each item is a deliverable work unit with clear acceptance criteria.

---

## Current State (v0.1.1)

| Feature | File | Notes |
|---|---|---|
| `garc init` | `main.rs` | Reads `.camp.toml`, writes hook, syncs branch |
| `garc checkout` | `main.rs` | Claim/settle/detect-collision/divert |
| `garc status` | `main.rs` | Reads mesh, shows occupied branches |
| Branch claim via mDNS | `mesh.rs` | Ephemeral `ServiceDaemon` per invocation |
| Peer discovery | `mesh.rs` | One-shot poll, configurable timeout |
| Collision engine | `engine.rs` | Lexicographical tie-breaking |
| Hook idempotency | `installer.rs` | `## BEGIN GARC MANAGED BLOCK` |
| JSON output | `output.rs` | All commands support `--json` |
| Config | `config.rs` | `.camp.toml` via `AgentConfig` + `DiscoveryConfig` |

### Gap Analysis vs. RAI Flywheel

| RAI Step | Status | Notes |
|---|---|---|
| Step 1 — Bootstrap (`camp init` + `garc init`) | OK | `garc init` exists and works |
| Step 2 — Mesh Presence (`camp up`) | **GAP** | No `garc up`; no delegation to `camp up` |
| Step 3 — Safe Checkout (`garc checkout`) | OK | Fully implemented; `camp update` is fire-and-forget |
| Steps 4-6 — `mask`, `wasp`, `tick` | N/A | Not GARC's responsibility |

---

## ITEM-01 — `garc up` command

**Priority:** High

**Why:** RAI Flywheel Step 2 is `camp up`. Agents that are only `garc`-aware have no
way to bring themselves onto the mesh after `garc init`. Without this, the RAI sequence
has a dead end.

**Design:** Thin wrapper — not a reimplementation of the mDNS advertisement daemon.

Steps inside `garc up`:
1. Read `.camp.toml`, validate git repo + project name match.
2. Sync current git branch into `.camp.toml` (same as `garc init` does).
3. Shell out to `camp up --config .camp.toml` — blocking, not fire-and-forget.
4. If `camp` binary not in `$PATH` — exit 1 with an install hint pointing to the installer script.
5. Propagate `--json` flag through if user passed it.

**Files to change:**
- `src/cli.rs` — add `Up(UpArgs)` enum variant and `UpArgs` struct
- `src/main.rs` — add `run_up()` function and match arm in `run()`
- `src/output.rs` — add `UpOutput` struct for the JSON preamble printed before delegating

**Acceptance criteria:**
- `garc up` blocks successfully when `camp` is in `$PATH`
- `garc up` exits 1 with friendly error message when `camp` is absent
- `garc up --json` prints a JSON preamble then delegates
- CLI integration test validates both paths using a mock `camp` stub

---

## ITEM-02 — `garc status`: `camp_status` field

**Priority:** Medium

**Why:** Agents have no single-command way to check whether their own mesh presence is
live. RAI Flywheel Step 2 requires `camp up` to be running; `garc status` should surface
this.

**New `camp_status` field in `StatusOutput`:**
- `"running"` — `camp` process is currently detectable (via `pgrep camp` or PID file)
- `"not_found"` — `camp` binary is not in `$PATH`
- `"unknown"` — process detection failed for another reason

**Files to change:**
- `src/output.rs` — add `camp_status: Option<String>` to `StatusOutput`
- `src/main.rs` — populate the field in `run_status()` before building `StatusOutput`

**Acceptance criteria:**
- `garc status --json` includes a `"camp_status"` field
- No test failures when `camp` binary is absent

---

## ITEM-03 — `garc checkout`: `camp_update_status` field

**Priority:** Medium

**Why:** After checkout, `garc` calls `camp update --branch <branch>` as a silent
fire-and-forget. Failures are swallowed. LLM consumers parsing `--json` output have no
visibility into whether the mesh was updated.

**New `camp_update_status` field in `CheckoutOutput`:**
- `"synced"` — `camp update` exited with code 0
- `"skipped"` — `camp` not in `$PATH` (not an error; offline use is supported)
- `"failed"` — `camp update` exited non-zero

**Files to change:**
- `src/output.rs` — add `camp_update_status: String` to `CheckoutOutput`
- `src/main.rs` — capture exit status in `update_local_state()` and return it to the caller

**Acceptance criteria:**
- `garc checkout <branch> --json` always includes `"camp_update_status"`
- Existing tests remain green after the field addition

---

## ITEM-04 — `garc commit` guard

**Priority:** Low

**Why:** The RAI architecture describes `garc` as intercepting both *checkout* and
*commit* commands. Currently only checkout is guarded. A lightweight `garc commit`
wrapper prevents agents from committing on a branch that has shifted under them.

**Behavior of `garc commit [-- <git_commit_args>...]`:**
1. Read `.camp.toml` and validate project name against git repo.
2. Run collision check using `detect_collision()` — if contested, exit 1 with JSON error.
3. If clear, execute `git commit <passthrough_args>` via `std::process::Command` (no shell).

**Files to change:**
- `src/cli.rs` — add `Commit(CommitArgs)` variant with `passthrough: Vec<String>`
- `src/main.rs` — add `run_commit()` function
- `src/output.rs` — add `CommitOutput` struct

**Acceptance criteria:**
- `garc commit -- -m "message"` succeeds when branch is clear
- `garc commit -- -m "message" --json` outputs a JSON result
- When branch is contested, exits 1 with a structured JSON error
- Git is invoked via `std::process::Command` — no shell injection

---

## ITEM-05 — Peer discovery via `camp list` shell-out

**Priority:** Medium

**Why:** `camp` supports HMAC-SHA256 shared-secret authentication. When agents run
`camp` with a shared secret, `garc`'s internal raw `ServiceDaemon` cannot validate their
signed TXT records — they appear as zero peers even when actively online.

**Chosen approach — prefer `camp list --json` over raw daemon:**

When `camp` is available in `$PATH`:
1. Call `camp list --json` via `std::process::Command`.
2. Parse stdout into `Vec<MeshPeer>` (the JSON schema matches what `camp` already emits).
3. Fall back to raw mDNS `ServiceDaemon` only when `camp` is absent.

Why shell-out instead of native HMAC in `garc`:
- Exact auth parity — whatever `camp` is configured with just works.
- Zero new crypto dependencies added to `garc`.
- Keeps `garc`'s own mDNS surface minimal.

**Files to change:**
- `src/mesh.rs` — add `discover_peers_via_camp()` function
- `src/mesh.rs` — update `discover_peers()` to call `discover_peers_via_camp()` first when `camp` is present
- `src/config.rs` — no structural changes; `shared_secret_mode` field already exists but is unused

**Acceptance criteria:**
- When `camp` in `$PATH`: `garc status --json` peer list matches `camp list --json`
- When `camp` absent: raw mDNS fallback produces results
- Both paths covered by tests using the existing `GARC_MESH_SNAPSHOT_JSON` env-var mock

---

## Delivery Order

```
ITEM-01  ->  ITEM-05  ->  ITEM-02  ->  ITEM-03  ->  ITEM-04
  (core)       (auth)       (obs)       (polish)     (guard)
```

- **ITEM-01 + ITEM-05** — must ship together to unblock the complete RAI Flywheel
- **ITEM-02 + ITEM-03** — improve situational awareness for operator and LLM consumers
- **ITEM-04** — lowest priority; can be deferred to a post-v0.2.0 patch

---

## Version Plan

| Item | Semver Impact | Reason |
|---|---|---|
| ITEM-01 | minor | new command added |
| ITEM-02 | patch | additive JSON field |
| ITEM-03 | patch | additive JSON field |
| ITEM-04 | minor | new command added |
| ITEM-05 | patch | internal discovery improvement, no API change |

**Target release: v0.2.0** (after ITEM-01 through ITEM-05)

---

## Non-Goals

- `garc` will **not** reimplement the `camp` heartbeat / TTL eviction daemon
- `garc` will **not** add extra persistence beyond `.git/garc/claim-state.json`
- `garc` will **not** implement crypto / authentication natively — delegated to `camp`
- `garc` will **not** manage remote branch lifecycle (push, PR creation, merges)

---

## References

- `PROJECTS.md` — RAI Flywheel and system architecture overview
- `coding_agent_mesh_presence` README — `camp` CLI surface and `.camp.toml` schema
- `src/mesh.rs` — current mDNS discovery and branch-claim implementation
- `src/cli.rs` — current command surface
