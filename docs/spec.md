You are an expert Rust systems engineer specializing in CLI tooling, Git internals, and distributed networking. I need you to implement `garc` (Git Agent Race Controller) — a production-quality Rust CLI binary based on the following technical specification.

---

**Technical Specification:**

```markdown
# Technical Specification: Git Agent Race Controller (`garc`)

## 1. Overview
`garc` is a lightweight, high-performance Rust CLI that acts as a concurrency gatekeeper for Git operations in a multi-agent environment.

By integrating directly with the `coding_agent_mesh_presence` (CAMP) network, `garc` prevents autonomous AI coding agents from stepping on each other's toes, overwriting shared files, or fighting over the same Git branch on a shared Local Area Network (LAN).

## 2. Motivation
In local Swarm Intelligence or multi-agent workflows, multiple LLMs (e.g., Claude, AutoGen workers) often operate on the same repository. Without coordination, Agent A might checkout `main` and modify `auth.rs`, while Agent B simultaneously checks out `main` and rewrites `auth.rs`. This results in race conditions, corrupted states, and broken automated tests.

`garc` solves this by introducing **Pessimistic Branch Concurrency** at the network level, requiring zero central servers.

The initial design only checked whether another peer was already broadcasting `current_branch=<target>`. That leaves a time-of-check/time-of-use gap: two agents can both observe a branch as clear, then both check it out before either one publishes its final branch state. This spec closes that gap with a short-lived claim handshake.

## 3. Architecture
The system operates as a wrapper and an observer, consisting of four primary modules:

1. **The CLI Proxy (`cli`):** The primary interface for AI agents. Agents are instructed via their system prompts to use `garc checkout` instead of `git checkout`.
2. **The Collision Engine (`engine`):** Evaluates local repository states against the real-time mDNS mesh registry (via CAMP) to determine if a branch is clear, actively occupied, or temporarily claimed.
3. **The Hook Injector (`installer`):** Manages `.git/hooks/post-checkout` to ensure that even if an agent bypasses `garc` and uses standard `git`, the mesh is still updated with their current branch.
4. **The Claim Broadcaster (`mesh` or equivalent support code):** Registers a short-lived mDNS advertisement on the CAMP service type while a checkout decision is in-flight, so competing agents can see each other's intent before a real checkout occurs.

## 4. Core Workflows

### 4.1. The Intercepted Checkout
When an agent executes `garc checkout feature-login`:

1. **Publish Claim:** GARC publishes a short-lived claim for `feature-login` on the CAMP service type before touching Git state. The claim must include `agent_id`, `current_project`, and `intent_branch=feature-login`.
2. **Settle and Re-read Mesh:** GARC waits briefly for the claim to become observable on the LAN, then re-reads the CAMP mesh snapshot.
3. **Path A (Clear / Won Arbitration):** If no peer in the same project is already broadcasting `current_branch=feature-login`, and the local agent wins deterministic claim arbitration, GARC uses `libgit2` to execute the checkout and updates the local agent's final branch state.
4. **Path B (Collision / Lost Arbitration):** If another peer in the same project is already on `feature-login`, or if another peer wins claim arbitration for `intent_branch=feature-login`, GARC **blocks** the direct checkout.
   - It automatically creates a scoped sub-branch: `feature-login--<local_agent_id>`.
   - It checks out the sub-branch.
   - It returns a structured output to `stdout` informing the LLM of the diversion.
5. **Release Claim:** After the checkout decision is complete, GARC stops advertising the temporary claim. If the process crashes mid-flight, the claim must disappear through normal mDNS TTL expiry instead of a central lock server.

### 4.2. Claim Arbitration Rules
A requested branch is unavailable when any peer in the same `current_project` meets either condition:

- `current_branch=<requested_branch>`
- `intent_branch=<requested_branch>`

Arbitration rules:

1. Active occupancy (`current_branch=<requested_branch>`) takes precedence over in-flight claims.
2. If multiple peers advertise `intent_branch=<requested_branch>` concurrently, the winner is the peer whose `agent_id` is lexicographically smallest.
3. The local agent participates in that comparison using its own `agent_id`.
4. Peers from other projects must be ignored.
5. The `--force` flag bypasses both active occupancy checks and claim arbitration.

This keeps the protocol deterministic without depending on wall-clock synchronization between hosts.

### 4.3. Git Hook Broadcasting (The Safety Net)
If an agent runs `garc init`, the tool installs a `post-checkout` shell hook into the repository.
Whenever *any* checkout occurs (even via standard `git`), the hook fires:
```bash
camp update --branch $(git rev-parse --abbrev-ref HEAD)
```
This hook remains a best-effort safety net for synchronizing steady-state branch metadata after checkouts. The claim handshake described above must not depend on a hypothetical CAMP `update intent` command.

## 5. mDNS / CAMP Metadata Contract
`garc` consumes CAMP-compatible services on the LAN. Required steady-state TXT fields remain:

- `agent_id`
- `current_project`
- `current_branch`

Optional transient TXT field introduced by this spec:

- `intent_branch`

Implementation note:

- The transient claim may be advertised either by enriching the agent's existing CAMP-compatible announcement or by registering a short-lived auxiliary service on the same CAMP service type.
- Consumers must trust TXT fields and `agent_id` / `current_project` semantics, not a specific DNS-SD instance naming convention for claim records.

## 6.1. Local Configuration Expectations
`garc` continues to read repo-local `.camp.toml` for local identity and discovery settings.

In addition to existing discovery settings, the implementation should support:

- `discovery.claim_settle_ms` (optional)
- `discovery.camp_rest_url` (optional)

Behavior:

- when omitted, GARC uses a safe default settle window
- when present, the value tunes how long GARC waits after publishing a claim before re-reading the mesh
- this knob affects safety/latency trade-offs only for claim arbitration; it must not change steady-state occupancy semantics
- when `discovery.camp_rest_url` is present, GARC may query that local CAMP REST bridge for peer discovery before falling back to raw mDNS

## 7. Command Line Interface (ACI - Agent Computer Interface)
`garc` is built for LLM parsing. All standard outputs are predictable, and JSON output is natively supported.

### `garc init`
Initializes the repository.
- Installs Git hooks.
- Reads `.camp.toml` to identify the local agent.

### `garc checkout <branch>`
The smart checkout wrapper.
- **Flags:**
  - `--force`: Bypasses the mesh check and claim arbitration (dangerous, human-use only).
  - `--claim-settle-ms <millis>`: Overrides the configured claim settle window for this invocation only.
  - `--json`: Outputs the result in JSON format for strict LLM parsing.
- **Output Example (Collision handled):**
  ```json
  {
    "status": "diverted",
    "requested_branch": "feature-login",
    "occupied_by": "qa-agent-01",
    "actual_branch": "feature-login--coder-01",
    "message": "Target branch is currently locked. Checked out sub-branch to prevent race conditions."
  }
  ```

The external CLI stays stable for the first iteration. Internally, `checkout` now performs claim publication, settle/re-read, arbitration, direct-or-diverted checkout, and claim release.

For richer agent/operator observability, checkout JSON should also support optional coordination fields such as:

- `decision_basis`
- `observed_claims`
- `claim_winner`
- `observed_peers`
- `decision_trace`
- `decision_trace_entries`
- `mesh_read_attempts`
- `mesh_read_backoff_ms`
- `camp_update_status`
- `camp_update_exit_code`
- `camp_update_stderr`

`observed_peers` should provide a concise machine-readable trace of the same-project peers that influenced the decision, including at least:

- `agent_id`
- `current_branch`
- `intent_branch` when present

`decision_trace` should provide a compact, ordered explanation of the coordination flow, for example claim publication, mesh re-read, arbitration outcome, and final checkout decision.

`decision_trace_entries` should provide a structured, timestamped variant of that same story. Each entry should include at least:

- `event`
- `at_ms`

where `at_ms` is a relative millisecond offset from the start of the checkout flow. This keeps the trace useful for debugging without requiring synchronized wall clocks across machines.

`mesh_read_attempts` should report how many mesh read attempts were needed after claim publication. `mesh_read_backoff_ms` should report the bounded backoff schedule used for those retries so operators can see whether a checkout succeeded immediately or only after jitter settled.

`camp_update_status` should continue to summarize whether the post-checkout mesh sync succeeded, was skipped, or failed. When available, `camp_update_exit_code` and `camp_update_stderr` should provide extra process-level detail for debugging failed sync attempts without forcing operators to rerun the command manually.

### `garc status`
Returns a unified view of the local Git status AND the mesh branch status.
Shows exactly who is working on what within the current repository.

Status output may include peers that expose `intent_branch`, but the command surface itself does not require a new subcommand or flag for the first iteration.

For operator visibility, `status --json` should expose transient claim information explicitly so a supervisor or agent can distinguish:

- steady-state occupancy (`current_branch`)
- in-flight claim intent (`intent_branch`)
- which peers are competing for the same branch

Recommended JSON summary sections:

- `occupied_branches`: same-project branches currently occupied by one or more peers
- `active_claims`: same-project claims currently visible on the mesh, including claimants and the deterministic claim winner when applicable
- `camp_status`: local CAMP availability/process signal for this machine

These summaries should include the local agent's current branch as part of the current project view. In other words, `status --json` should expose a project-level picture, not only a remote-peer picture.

When a local checkout is actively in the claim-handshake phase, `status --json` should also surface that **local in-flight claim** in the project summary view instead of hiding it until the final branch checkout completes.

`camp_status` should distinguish at least:

- `running` when local CAMP tooling is available and a local CAMP process appears to be running
- `not_found` when `camp` is not available in `$PATH`
- `unknown` when local probing fails for another reason, or when the binary is present but no reliable running-process signal is available

For local post-mortem debugging, GARC may also persist checkout traces under the repository's `.git/garc/` state directory. That persisted trace data is local-only observability state, not a coordination primitive.

Recommended local files:

- `.git/garc/last-checkout-trace.json` — overwrite-only pointer to the most recent checkout trace
- `.git/garc/trace-history/` — bounded history of recent checkout traces for debugging multiple attempts over time

### `garc trace`
Reads persisted local checkout trace data for the current repository.

Recommended behaviors:

- default mode returns the most recent persisted checkout trace
- `--history` returns a bounded list of recent persisted traces
- `--limit <n>` further caps the returned history length when `--history` is used
- `--json` emits stable machine-readable output

Recommended JSON fields:

- `status`
- `latest` for the most recent trace when available
- `history` for the ordered recent trace list when requested

Suggested statuses:

- `ok` when at least one persisted trace is available
- `empty` when no persisted trace exists yet

If no persisted trace exists yet, the command should return a structured empty response rather than failing.
If `--limit` is provided together with `--history`, the `history` array should contain at most `n` entries ordered from newest to oldest.

## 8. Edge Cases & Constraints

- **Orphaned Locks / Claims:** If a remote agent crashes, steady-state `current_branch` occupancy and short-lived `intent_branch` claims must disappear through CAMP/mDNS TTL eviction. GARC must not introduce a separate lock server.
- **Merge Conflicts on Sub-branches:** GARC prevents *checkout* and *write* collisions. It does not resolve Git merge conflicts. A separate Reviewer Agent must merge the `feature-login--coder-01` branch back into the main `feature-login` branch.
- **Auxiliary Claim Service Naming:** If GARC uses a temporary auxiliary mDNS record for claims, it must still publish the canonical `agent_id` in TXT metadata so collision decisions are based on the logical agent identity rather than a transport-level instance suffix.
- **Claim Publication Failure Policy:** In non-`--force` mode, if GARC cannot publish its temporary claim or cannot re-read the mesh after publishing it, the checkout should fail closed rather than silently performing an unsafe direct checkout.
- **Project Namespace Guardrail:** GARC should continue treating the repo-local `.camp.toml` project as part of the safety boundary. If the configured project does not match the repository project namespace, mesh-aware operations should stop with a structured error rather than risk cross-project coordination mistakes.
- **Status Summary Semantics:** Summary fields in `status --json` must be derived only from same-project peers. Other projects must never pollute branch occupancy or claim summaries.
- **Local CAMP Probe Hygiene:** Local CAMP availability detection must not write to stdout/stderr in ways that corrupt `--json` output.
- **Process Probe Scope:** `camp_status` should be based on local process/tooling signals only. It must not infer “running” merely from remote peers being visible on the mesh.
- **REST Bridge Preference:** If a local CAMP REST bridge URL is configured, GARC should prefer it for peer reads before spinning up a one-shot discovery daemon.
- **Local Claim Visibility:** If GARC persists local in-flight claim state for observability, that state must be ephemeral and automatically removed when the checkout decision completes or the process exits normally.
- **Persisted Trace Scope:** Any persisted checkout trace file must be treated as local debugging state only. It must not participate in arbitration.
- **Trace History Retention:** If GARC keeps a local trace history, it should prune older entries to a bounded count rather than growing unbounded inside `.git/`.
- **Most Recent Trace Pointer:** `last-checkout-trace.json` should always reflect the newest recorded checkout trace, even when history retention is enabled.
- **Override Precedence:** If `--claim-settle-ms` is provided, it overrides `.camp.toml` for that invocation only and must not rewrite the config file.
- **Mesh Re-read Reliability:** After publishing a claim, GARC may retry mesh re-read a small number of times before failing closed. Retries must prefer safety and determinism over speed.
- **Retry Backoff Policy:** Mesh re-read retries should use a bounded backoff schedule rather than a tight constant loop, so local LAN jitter can settle without turning one checkout into a long stall.
- **Performance:** The claim handshake should add only a short settle window appropriate for LAN discovery. Correctness is more important than chasing a < 2ms idealized path.

## 9. Verification Requirements
The implementation must include automated coverage for at least the following:

- direct checkout when the branch is clear and the local agent wins claim arbitration
- diverted checkout when another peer already advertises `current_branch=<requested_branch>`
- diverted checkout when another peer wins `intent_branch` arbitration
- direct checkout when another peer claims the branch but loses lexicographic arbitration
- same-project filtering for both occupancy and intent claims
- `--force` bypassing both occupancy and claim checks
- `claim_settle_ms` config parsing / default behavior
- `camp_rest_url` config parsing / discovery behavior
- `--claim-settle-ms` CLI override precedence
- status JSON summaries for occupied branches and active claims
- `status --json` `camp_status` field behavior
- status summaries including the local agent branch
- local in-flight claim visibility in status summaries
- checkout JSON `observed_peers` trace shape
- checkout JSON `decision_trace` shape
- checkout JSON `decision_trace_entries` timestamped shape
- checkout JSON `mesh_read_attempts` / `mesh_read_backoff_ms` fields
- checkout JSON `camp_update_exit_code` / `camp_update_stderr` fields
- retry/backoff helper behavior
- persisted last-checkout trace file lifecycle
- bounded trace-history pruning behavior
- `garc trace` latest/history output behavior
- `garc trace --limit <n>` history truncation behavior
- hook installer idempotence remains intact

## 10. Future Roadmap
- **Arbitration Observability:** Extend checkout/status JSON further with richer peer snapshots or historical traces if operators need more than the current structured summaries.
- **Claim Tuning:** Expand beyond a single `claim_settle_ms` knob if LAN-heavy environments later need adaptive settle windows or retry policies.
- **Claim Record Hygiene:** Consider a dedicated claim instance naming convention and/or explicit claim lifecycle markers if operational debugging on crowded meshes becomes difficult.
- **File-Level Locking:** Expand the mDNS payload to broadcast specific files currently open in the agent's editor/sandbox, preventing simultaneous writes to the same file even across different branches.
- **Auto-Merge Coordinator:** Integrate with `mask` (Mesh Agent Shared Knowledge) to automatically attempt `--ff-only` merges between agent sub-branches when they drop to an `idle` state.
```

---

**Implementation requirements:**

- Produce a complete, compilable Rust project with the full directory structure: `Cargo.toml`, `src/main.rs`, and any necessary submodules (`src/cli.rs`, `src/engine.rs`, `src/installer.rs`, etc.)
- Use `clap` (v4) for CLI argument parsing, `git2` (libgit2 bindings) for all Git operations, `serde` + `serde_json` for JSON output, and `mdns-sd` or `zeroconf` for mDNS mesh interaction. Choose the most appropriate and actively maintained crates.
- Model the CAMP mDNS service as a discoverable service type (e.g. `_camp._tcp.local`) with TXT records carrying `agent_id`, `current_branch`, `current_project`, and an optional transient `intent_branch` field.
- Support `discovery.claim_settle_ms` in `.camp.toml` as an optional claim-arbitration tuning knob.
- Support `--claim-settle-ms` as a per-command override for checkout claim arbitration timing.
- Use a bounded retry/backoff strategy when re-reading the mesh after claim publication.
- Persist the most recent checkout coordination trace under `.git/garc/` for local debugging, without using that file as an input to arbitration.
- Retain a bounded local history of recent checkout traces under `.git/garc/trace-history/`.
- Provide a `garc trace` command for reading persisted trace state without requiring operators to open `.git/garc/` manually.
- The `engine` module must implement the collision detection logic cleanly — query the mesh, compare `current_project` against the local repo name, and return typed checkout/arbitration decisions rather than ad-hoc booleans.
- All `garc checkout` output paths (clear, diverted, forced) must serialize to the JSON schema shown in the spec when `--json` is passed.
- `status --json` should provide machine-readable branch occupancy and active-claim summaries for the current project.
- GARC may persist ephemeral local claim state only for observability / status reporting, not as a substitute for CAMP TTL-based network coordination.
- Implement proper error handling using `anyhow` or `thiserror` — no `unwrap()` calls in production paths.
- The `post-checkout` hook written by `garc init` must be idempotent — running `garc init` twice must not duplicate hook entries.
- Include inline code comments explaining non-obvious design decisions, particularly around the mDNS TTL eviction behavior, claim arbitration, and the sub-branch naming convention.
