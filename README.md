# git_agent_race_controller (garc)

Zero-configuration Git branch collision detection and pessimistic concurrency resolution for multi-agent systems using mDNS/DNS-SD.

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](#license)
[![Rust](https://img.shields.io/badge/rust-stable-orange.svg)](#installation)
[![crates.io](https://img.shields.io/crates/v/garc.svg)](https://crates.io/crates/garc)
[![CI](https://github.com/0xBoji/git_agent_race_controller/actions/workflows/ci.yml/badge.svg)](https://github.com/0xBoji/git_agent_race_controller/actions/workflows/ci.yml)

> Think of `git_agent_race_controller` (`garc`) as a smart, network-aware `git checkout` wrapper:
> Before modifying your working tree, it checks the local LAN (via `camp` metadata) to see if another agent is already making changes to that exact branch. If they are, it automatically diverts you to a safe, agent-specific sub-branch.

---

## Table of Contents

- [What this crate is](#what-this-crate-is)
- [Why it exists](#why-it-exists)
- [Who should use it](#who-should-use-it)
- [Who should not use it](#who-should-not-use-it)
- [Status](#status)
- [TL;DR Quickstart](#tldr-quickstart)
- [Installation](#installation)
- [The mental model](#the-mental-model)
- [Core concepts](#core-concepts)
- [Typical workflows](#typical-workflows)
  - [1. Standard non-colliding checkout](#1-standard-non-colliding-checkout)
  - [2. Handling a blocked branch gracefully](#2-handling-a-blocked-branch-gracefully)
  - [3. Bypassing the mesh with `--force`](#3-bypassing-the-mesh-with---force)
  - [4. Observing the mesh state](#4-observing-the-mesh-state)
- [CLI commands reference](#cli-commands-reference)
- [Integration with CAMP](#integration-with-camp)
- [Git Hooks Management](#git-hooks-management)
- [Failure modes and edge cases](#failure-modes-and-edge-cases)
- [JSON output for LLMs & scripts](#json-output-for-llms--scripts)
- [Limitations and non-goals](#limitations-and-non-goals)
- [Testing and verification](#testing-and-verification)
- [Design notes](#design-notes)
- [Roadmap / likely next improvements](#roadmap--likely-next-improvements)
- [License](#license)

---

## What this crate is

`garc` is a Rust command-line tool for **branch-level pessimistic concurrency**.

It helps a set of parallel agents, workers, or tools operating on the same LAN and acting upon the same Git repositories:
- Prevent race conditions when checking out and modifying the same Git branches.
- Communicate their "checkout intents" using `camp` (Zero-configuration LAN service discovery).
- Settle checkout claims deterministically across the network without a central lock server.
- Fallback safely to a diverted branch (`<branch>--<agent_id>`) when an intended branch is currently occupied by a peer.

At the current MVP level, `garc` provides answers and strict guardrails for questions like:
- "Am I safe to check out `feature/mesh` without stomping on another agent's work?"
- "Who is currently occupying the branch I want to work on?"
- "If multiple agents try to check out `main` at the exact same millisecond, who wins?"

---

## Why it exists

When you run multiple autonomous coding agents in parallel, the repetitive and most error-prone part is Git state management. Git is natively decentralized, but it assumes a single actor per working tree. When you have five LLM-driven agents actively collaborating on the same codebase, Git branch collisions become a constant liability.

Without a shared, enforced lock layer, you end up with some combination of:
- agents silently force-pushing over each other.
- duplicated git history from conflicting base commits.
- complex external lock-servers that go down, become stale, or require constant authentication.
- ad-hoc shell scripts detecting unstaged changes that easily fail edge cases.

`garc` ensures safe concurrency by relying on the already existing `camp` presence layer:
- **automatic** on a shared LAN.
- **pessimistic lock** logic enforced before a checkout is even attempted locally.
- **deterministic** arbitration in case of a simultaneous race.
- **json-native** outputs tailored for LLM text parsers and orchestrators.

---

## Who should use it

This crate is a good fit if you are building:
- Local multi-agent developer tools that automatically manage git state.
- Workstation-side autonomous coders that rapidly iterate and branch from `main`.
- Continuous Integration scenarios where multiple test agents generate changes concurrently on a single subnet.
- Teams of humans and bots collaborating intensely on the same LAN repository where communication overhead is high.

It is heavily optimized for scenarios where you want the mesh to solve collisions quickly instead of halting execution or forcing the user to manually intervene.

---

## Who should not use it

This is **not** the right tool if you need:
- Absolute global locking for geographically distributed users across the WAN.
- Cloud-level branch protection (use GitHub's branch protection rules or GitLab's protected branches instead).
- A general-purpose git client visualization, GUI, or commit history inspector.
- File-level locking (like `git-lfs` locking). `garc` operates strictly at the macro branch level.

If your environment spans across VPN boundaries without mDNS relaying, or if you require strictly cryptographically signed centralized locks, `garc` is the wrong tool.

---

## Status

Current implementation includes:
- Synchronous pre-checkout probe for existing branch occupancy.
- Intention-publishing (`intent_branch`) through mDNS TXT records.
- Deterministic claim settling with a configurable `claim_settle_ms` window.
- Diverted local checkout engine (`branch--agent_id`) when collision is detected.
- Native integration with a repository-local `.camp.toml` configuration.
- Robust Git `post-checkout` hook management that safely integrates with manual `git checkout` actions.
- `--json` formatting for all lifecycle outputs.

This crate is intended for **local-network use only**.

---

## TL;DR Quickstart

If you just want the shortest path to making your repository safely accessible by multiple agents:

```bash
# 1. Initialize garc for your repository (must have already run `camp init`)
garc init

# 2. As an agent, attempt to check out a work branch:
garc checkout feature-login

# 3. If another agent is ALREADY on `feature-login`, garc will output:
#    "Target branch is currently locked. Checked out feature-login--coder-01 to prevent race conditions."

# 4. Want to see what the mesh looks like for this repo?
garc status
```

That gets you:
- an installed git hook that syncs your local `.camp.toml` automatically upon human `git checkout` invocations.
- pessimistic locking when agents invoke `garc checkout`.
- safe fallbacks so nobody's git context gets ruined by a concurrent peer.

---

## Installation

When the crate is published, the standard Cargo path is:

```bash
cargo install garc
```

If you prefer a curl/bash installer (which guarantees the binary is installed globally without needing a Rust toolchain):

```bash
bash <(curl -fsSL https://raw.githubusercontent.com/0xBoji/git_agent_race_controller/main/scripts/install.sh)
```

And if you want to install directly from GitHub before the crates.io package is available (useful for bleeding-edge features):

```bash
bash <(curl -fsSL https://raw.githubusercontent.com/0xBoji/git_agent_race_controller/main/scripts/install.sh) --git
```

The installer intelligently tries crates.io first, and correctly falls back to GitHub releases if it detects unreleased versions.

---

## The mental model

The easiest way to reason about `garc` is to imagine it as a polite, robotic traffic controller at the entrance of a one-way street (the Git branch).

1. **Identity & Context**: `garc` reads `.camp.toml` to know who you are (`agent_id`) and what project namespace you are operating in.
2. **The Intention**: Rather than immediately driving down the street, `garc` raises a flag via `camp` networking saying: *"I intent to checkout `target_branch`"*.
3. **The Pause**: `garc` waits for a short duration (`claim_settle_ms`). This handles the scenario where two agents arrive at the exact same millisecond.
4. **The Observation**: `garc` surveys the mesh. Is someone else already on the street (`current_branch == target_branch`)? Or did someone else raise their flag *at the same time*?
5. **The Arbitration**: If there's a tie, they compare names. Lower ascii name wins. The loser gives up peacefully.
6. **The Result**: 
   - **Clear**: `garc` drives into `git checkout target_branch`.
   - **Occupied/Lost**: `garc` takes a detour into `git checkout -b target_branch--your_agent_id`.
7. **The Update**: Once parked, the `post-checkout` hook commits your new location to the mesh so others know you are there.

You do **not** manually manage:
- the branch check script logic.
- the mDNS lookup timing.
- the fallback branch creation command.
- locking states in separate text files or databases.

---

## Core concepts

### Collision Detection
Before executing a git process, `garc` polls the LAN for any peer whose `project` matches the current repo, and whose `current_branch` is identical to your requested target.

### Intent Publishing (`intent_branch`)
A split-second before checking out, an agent will publish an `intent_branch` metadata tag to the mesh. This warns peers that are simultaneously querying the network that a checkout is imminent, preventing interleaved race conditions.

### Claim Arbitration
If two agents observe each other's `intent_branch` for the exact same target simultaneously, `garc` resorts to **Lexicographical tie-breaking**. If agent `alpha-01` and `beta-02` clash, `alpha-01` deterministically wins. `beta-02` recognizes it lost, outputs an arbitration failure trace, and diverges its branch.

### Branch Diversion
If `garc` determines a target is off-limits, it does not throw an error. In multi-agent systems, throwing an error usually halts an LLM orchestration loop. Instead, `garc` gracefully diverts the checkout target to `<requested_branch>--<agent_id>`, creating it from the current `HEAD` if necessary. This allows the agent to continue generating code safely.

---

## Typical workflows

### 1. Standard non-colliding checkout

When the network is clear, `garc` operates effectively identically to `git checkout`:

```bash
$ garc checkout feat/database

Checked out 'feat/database'
Decision basis: MeshClear
Mesh trace:
  - published_claim:feat/database
  - claim_settle_ms:150
  - mesh_clear
  - decision:checked_out
```

### 2. Handling a blocked branch gracefully

If `worker-01` is already on the branch you want:

```bash
$ garc checkout main

Target branch is currently locked. Checked out sub-branch to prevent race conditions.
Requested: main
Actual: main--coder-01
Occupied by: worker-01
Decision basis: BranchOccupied
```

This ensures the agent does not overwrite `worker-01`'s unstaged work if they somehow share a filesystem, or more likely, guarantees their subsequent `git push` will not conflict.

### 3. Bypassing the mesh with `--force`

If you are a human and you deliberately want to switch branches regardless of what autonomous agents are doing, you can bypass the lock:

```bash
$ garc checkout main --force

Bypassed mesh collision checks and checked out the requested branch.
Decision basis: ForceBypass
```

### 4. Observing the mesh state

Agents or managers can inspect exactly what's happening:

```bash
$ garc status

Agent ID:      coder-01
Project:       lumen-core
Local branch:  main

Occupied Branches:
  - main (coder-01, worker-01)
  - feat/router (reviewer-02)

Active Claims: None
```

---

## CLI commands reference

### `garc init`

```bash
garc init
garc init --json
garc init --config path/to/.camp.toml
```

Verifies the git repository, reads `.camp.toml`, ensures the `agent.project` matches the repository root directory name, checks out the current branch to `camp`'s configuration, and finally overwrites `.git/hooks/post-checkout` with an idempotent hook block.

### `garc checkout <branch>`

```bash
garc checkout <branch>
garc checkout <branch> --json
garc checkout <branch> --force
garc checkout <branch> --claim-settle-ms 500
```

Executes the claim logic. The `--claim-settle-ms` override can be used if your local network mDNS discovery is particularly slow and you want to reduce the risk of missed collision detection in race scenarios.
The override only applies to that single checkout attempt; it does not rewrite `.camp.toml`.

### `garc status`

```bash
garc status
garc status --json
garc status --config path/to/.camp.toml
```

Shows the current local branch, actively occupied branches on the mesh, and any pending `intent_branch` claims.
If a local checkout is currently mid-claim, the project-level JSON summary includes that local in-flight claim as well.

### `garc trace`

```bash
garc trace
garc trace --json
garc trace --history --json
```

Reads persisted local checkout traces from `.git/garc/`. Use it when you want to inspect the latest arbitration result or a bounded recent history without opening the state files manually.

---

## Integration with CAMP

`garc` is tightly coupled with `coding_agent_mesh_presence` (`camp`). 
It does not re-implement an mDNS daemon. Instead, it expects:
1. A valid `.camp.toml` in your repository.
2. The `camp` binary to be available in your system `$PATH`.

When `garc` diverts or checks out a branch, it actively calls `camp update --branch <actual_branch>` under the hood to ensure the mesh is immediately hydrated with the new presence state.

Minimal expected fields in `.camp.toml`:
```toml
[agent]
id = "coder-01"
project = "lumen-core"
branch = "main"

[discovery]
service_type = "_camp._tcp.local."
```

If `garc` detects a mismatch between your git repository's directory name and `agent.project`, it will exit with an error. This prevents agents working in `repo_A` from colliding with branch names in `repo_B` simply because they are on the same machine.

---

## Git Hooks Management

### `post-checkout` idempotency
`garc init` injects a block into `.git/hooks/post-checkout`.
This block is wrapped in clear `## BEGIN GARC MANAGED BLOCK` markers. `garc` will not append the block twice. Run `garc init` as often as you want. Any pre-existing user hooks are preserved safely.

### Safety fallbacks
The injected hook is designed to capture *human* `git checkout` actions. If a human bypasses `garc` and runs `git checkout dev`, the post-checkout hook fires, calling:
`camp update --branch "$(git rev-parse --abbrev-ref HEAD)"`
This ensures the mesh remains accurate even if the users forget to use the CLI wrapper.

---

## Failure modes and edge cases

### Agent crashing during intent
If an agent crashes mid-execution (after publishing `intent_branch` but before settling), `garc` manages this via `camp`'s native TTL eviction. The orphaned claim will eventually evaporate from the LAN.

### Missing local branch on divergent repo
If an agent loses arbitration and is diverted to `<branch>--<agent_id>`, but the original target `<branch>` does not exist locally (e.g. they checked out a remote tracking branch), `garc` creates the diverted branch starting from `HEAD` (the current branch they were on).

### Network Drops
If the mDNS daemon cannot bind to a port, `garc` will gracefully fail with a verbose `thiserror` driven message. If the mesh is empty (no other `camp` peers found), `garc` assumes it is alone and executes checkout normally.

---

## JSON output for LLMs & scripts

`garc` is fundamentally designed to be operated by a language model. The `--json` flag formats all output into reliable schema.

Example `garc checkout feat --json` (diverted state):
```json
{
  "status": "diverted",
  "requested_branch": "feat",
  "occupied_by": "qa-agent-01",
  "decision_basis": "BranchOccupied",
  "observed_claims": [],
  "observed_peers": [
    {
      "agent_id": "qa-agent-01",
      "current_branch": "feat",
      "intent_branch": null
    }
  ],
  "claim_winner": null,
  "mesh_read_attempts": 1,
  "mesh_read_backoff_ms": [],
  "decision_trace": [
    "published_claim:feat",
    "claim_settle_ms:150",
    "claim_settle_complete",
    "observed_peer_count:1",
    "mesh_read_attempts:1",
    "active_occupier:qa-agent-01",
    "decision:diverted:feat--coder-01"
  ],
  "decision_trace_entries": [
    {
      "event": "published_claim:feat",
      "at_ms": 0
    },
    {
      "event": "decision:diverted:feat--coder-01",
      "at_ms": 151
    }
  ],
  "actual_branch": "feat--coder-01",
  "message": "Target branch is currently locked. Checked out sub-branch to prevent race conditions."
}
```

LLMs can parse the `actual_branch` to immediately know where to perform their `git commit` and `git push` commands. The `decision_trace` array provides immediate debuggability for agent reasoning chains, while `decision_trace_entries` adds relative timing context for post-mortem debugging.
GARC also persists the most recent checkout trace under `.git/garc/last-checkout-trace.json` for local debugging.
In addition, it keeps a bounded recent history under `.git/garc/trace-history/`.
These persisted traces are local-only observability state; they are not consulted during future arbitration.

---

## Limitations and non-goals

- It only checks for branch names. It does not look at the git commit DAG or resolve merge conflicts.
- It is strictly optimistic until the point of checkout. It doesn't lock a branch *while* you type code, only when you switch context. (Once you are on the branch, your presence *acts* as a lock against others trying to check into it).
- Does not operate across VLANs gracefully unless an mDNS repeater is configured on your router.

---

## Testing and verification

The repository is built with extreme robustness in mind for multi-agent loops.

Run the test suite:
```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

The `tests` directory includes:
- Unit tests for collision detection logic and branch string sanitization.
- Hook installer idempotency tests involving complex pre-existing bash scripts.
- Serialization tests guaranteeing the JSON output format never changes unobservably.
- CLI integration tests mocking the `.camp.toml` structure.

---

## Design notes

- **Settling Window**: The default claim settle window is deliberately low (typically 150ms) to provide a snappy shell experience for humans, while still offering enough breadth to catch network races for agents operating simultaneously.
- **Why pessimisim?**: Agent orchestration is inherently chaotic. The cost of recovering from a detached HEAD or a force-pushed history diverge is very high. The cost of creating a `feature--agent` branch is merely a `git merge` away from resolution. So, `garc` defaults to defensive branch creation.
- **Stateless engine**: `garc` itself has zero background daemons. The engine is entirely ephemeral, executing, arbitrating, modifying `.git`, and exiting in under 200 milliseconds.

---

## Roadmap / likely next improvements

- Configurable tie-breaking logic (e.g. allowing `role="human"` to always win claim arbitration against an agent).
- Stricter enforcement preventing commits via a `pre-commit` hook if the branch state shifted underneath the agent.
- Optional interactive mode for humans (`Branch occupied. Divert to feat-1? [Y/n]`).

---

## License

This project is licensed under the MIT License - see the LICENSE file for details.
