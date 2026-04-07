# garc

`garc` (Git Agent Race Controller) is a Rust CLI that wraps Git branch checkout with CAMP-aware collision detection.

It is designed for multi-agent coding setups on a shared LAN where multiple autonomous agents might otherwise check out and overwrite the same branch at the same time.

## Install

From crates.io once published:

```bash
cargo install garc
```

If you want the same curl/bash install flow as `camp`:

```bash
bash <(curl -fsSL https://raw.githubusercontent.com/0xBoji/git_agent_race_controller/main/scripts/install.sh)
```

And if you want to install directly from GitHub:

```bash
bash <(curl -fsSL https://raw.githubusercontent.com/0xBoji/git_agent_race_controller/main/scripts/install.sh) --git
```

The installer tries crates.io first, then falls back to GitHub automatically.

## What it does

- checks the local CAMP mesh before `checkout`
- allows direct checkout when the target branch is clear
- diverts to `<branch>--<agent_id>` when another peer already occupies the branch
- installs an idempotent `post-checkout` hook so plain `git checkout` can still trigger CAMP sync
- emits predictable JSON for LLMs and scripts with `--json`

## Commands

### `garc init`

Installs the managed `post-checkout` hook and synchronizes the local `.camp.toml` branch value with the current repository branch.

```bash
garc init
garc init --json
```

### `garc checkout <branch>`

Performs a CAMP-aware checkout.

```bash
garc checkout feature-login
garc checkout feature-login --json
garc checkout feature-login --force
```

Possible behaviors:

- **clear**: checks out `feature-login`
- **diverted**: checks out `feature-login--<local_agent_id>`
- **forced**: bypasses mesh collision checks

Example diverted JSON:

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

Shows the current local branch plus discovered CAMP peers for the same project.

`garc` treats `.camp.toml` as the project source of truth and validates that
`agent.project` matches the repository directory name before performing mesh-aware
operations. This avoids silently mixing unrelated repos into the same branch
collision namespace.

```bash
garc status
garc status --json
```

## CAMP expectations

`garc` expects a repo-local `.camp.toml` file, typically created by `camp init`.

Minimal expected fields:

- `agent.id`
- `agent.project`
- `agent.branch`
- `discovery.service_type` (defaults to `_camp._tcp.local.` if omitted)

Optional discovery override:

- `discovery.mdns_port` for isolated local testing when you want `garc` to browse a non-default mDNS port

Mesh peer discovery reads these TXT records from mDNS/DNS-SD:

- `agent_id`
- `current_branch`
- `current_project`

## Hook behavior

`garc init` writes a managed block into `.git/hooks/post-checkout`:

- it preserves unrelated existing hook content
- it does not duplicate the `garc` block across repeated runs
- it shells out to `camp update --branch "$(git rev-parse --abbrev-ref HEAD)"` as a safety net when users bypass `garc`

## Design notes

- Collision handling is **branch-level pessimistic concurrency**.
- Orphaned locks are expected to disappear through CAMP/mDNS TTL eviction rather than a central lock server.
- If the requested branch is occupied and not available locally or on `origin`, the diverted branch is created from the current `HEAD` as an explicit fallback.

## Development

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

## Test coverage

The repository currently includes:

- unit tests for collision detection and branch sanitization
- unit tests for hook installer idempotence
- unit tests for JSON output shape
- unit tests for git checkout/diversion behavior
- CLI integration tests for:
  - `init --json`
  - clear `checkout --json`
  - diverted `checkout --json`
  - `status --json`
  - structured JSON errors when `.camp.toml` is missing
