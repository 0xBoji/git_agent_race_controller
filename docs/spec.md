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

## 3. Architecture
The system operates as a wrapper and an observer, consisting of three primary modules:

1. **The CLI Proxy (`cli`):** The primary interface for AI agents. Agents are instructed via their system prompts to use `garc checkout` instead of `git checkout`.
2. **The Collision Engine (`engine`):** Evaluates local repository states against the real-time mDNS mesh registry (via CAMP) to determine if a branch is "occupied".
3. **The Hook Injector (`installer`):** Manages `.git/hooks/post-checkout` to ensure that even if an agent bypasses `garc` and uses standard `git`, the mesh is still updated with their current branch.

## 4. Core Workflows

### 4.1. The Intercepted Checkout
When an agent executes `garc checkout feature-login`:

1. **Query Mesh:** GARC polls the local CAMP registry: *"Is any peer currently broadcasting `current_branch=feature-login` in `current_project=my-repo`?"*
2. **Path A (Clear):** If no peer is on the branch, GARC uses `libgit2` to execute the checkout and updates the local agent's mDNS broadcast.
3. **Path B (Collision):** If `Agent-02` is currently on `feature-login`, GARC **blocks** the direct checkout. 
    - It automatically creates a scoped sub-branch: `feature-login--<local_agent_id>`.
    - It checks out the sub-branch.
    - It returns a structured output to `stdout` informing the LLM of the diversion.

### 4.2. Git Hook Broadcasting (The Safety Net)
If an agent runs `garc init`, the tool installs a `post-checkout` shell hook into the repository. 
Whenever *any* checkout occurs (even via standard `git`), the hook fires:
```bash
camp update --branch $(git rev-parse --abbrev-ref HEAD)
```
This guarantees the mesh state is always strictly synchronized with the actual file system state.

## 5. Command Line Interface (ACI - Agent Computer Interface)

`garc` is built for LLM parsing. All standard outputs are predictable, and JSON output is natively supported.

### `garc init`
Initializes the repository.
- Installs Git hooks.
- Reads `.camp.toml` to identify the local agent.

### `garc checkout <branch>`
The smart checkout wrapper.
- **Flags:**
  - `--force`: Bypasses the mesh check (dangerous, human-use only).
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

### `garc status`
Returns a unified view of the local Git status AND the mesh branch status.
Shows exactly who is working on what within the current repository.

## 6. Edge Cases & Constraints

- **Orphaned Locks:** If a remote agent crashes and fails to update the mesh, its branch might appear "locked". GARC relies on the CAMP TTL (Time-To-Live) eviction policy. Once the crashed agent's mDNS heartbeat expires, the branch is automatically unlocked.
- **Merge Conflicts on Sub-branches:** GARC prevents *checkout* and *write* collisions. It does not resolve Git merge conflicts. A separate Reviewer Agent must merge the `feature-login--coder-01` branch back into the main `feature-login` branch.
- **Performance:** Network queries to the local in-memory CAMP registry take < 2ms, ensuring no noticeable latency is added to standard Git operations.

## 7. Future Roadmap
- **File-Level Locking:** Expand the mDNS payload to broadcast specific files currently open in the agent's editor/sandbox, preventing simultaneous writes to the same file even across different branches.
- **Auto-Merge Coordinator:** Integrate with `mask` (Mesh Agent Shared Knowledge) to automatically attempt `--ff-only` merges between agent sub-branches when they drop to an `idle` state.
```

---

**Implementation requirements:**

- Produce a complete, compilable Rust project with the full directory structure: `Cargo.toml`, `src/main.rs`, and any necessary submodules (`src/cli.rs`, `src/engine.rs`, `src/installer.rs`, etc.)
- Use `clap` (v4) for CLI argument parsing, `git2` (libgit2 bindings) for all Git operations, `serde` + `serde_json` for JSON output, and `mdns-sd` or `zeroconf` for mDNS mesh interaction. Choose the most appropriate and actively maintained crates.
- Model the CAMP mDNS service as a discoverable service type (e.g., `_camp._tcp.local`) with TXT records carrying `agent_id`, `current_branch`, and `current_project` fields.
- The `engine` module must implement the collision detection logic cleanly — query the mesh, compare `current_project` against the local repo name, and return a typed `CollisionResult` enum (`Clear` or `Occupied { by: String }`).
- All `garc checkout` output paths (clear, diverted, forced) must serialize to the JSON schema shown in the spec when `--json` is passed.
- Implement proper error handling using `anyhow` or `thiserror` — no `unwrap()` calls in production paths.
- The `post-checkout` hook written by `garc init` must be idempotent — running `garc init` twice must not duplicate hook entries.
- Include inline code comments explaining non-obvious design decisions, particularly around the mDNS TTL eviction behavior and the sub-branch naming convention.