# Architecture

This document explains how `herdr-spreader` is put together: the module boundaries, how data flows from a YAML file to actual `herdr` CLI invocations, and the non-obvious design decisions — most of which exist because of bugs found during review, not because they were planned upfront.

## Module map

```
src/
├── main.rs        thin CLI entry point: wires config → engine → backend together
├── cli.rs         clap argument parsing (`apply --file <path>`)
├── config.rs       YAML → SpreadFile (a list of Workspaces), plus all path resolution (root/cwd/tilde)
├── engine.rs       pure expansion logic: SpreadFile → sequence of HerdrBackend calls
├── backend/
│   ├── mod.rs      the HerdrBackend trait and its Opts/Created/Error types
│   └── cli.rs      CliBackend: HerdrBackend implemented by spawning the herdr binary
└── lib.rs          re-exports the above as a library, so tests/ can link against it
```

The dependency direction is strictly one-way:

```
main.rs ──▶ cli.rs
main.rs ──▶ config.rs
main.rs ──▶ engine.rs ──▶ backend/mod.rs (trait only)
main.rs ──▶ backend/cli.rs ──implements──▶ backend/mod.rs
```

`engine.rs` depends only on the `HerdrBackend` **trait**, never on `CliBackend` directly. That's what makes the engine's expansion logic testable without spawning real processes (see [Testing strategy](#testing-strategy)).

## Data flow

```
spread.yml (workspaces: list, required)
    │  config::load_config → fs::read_to_string + SpreadFile::from_str (serde_yaml_ng)
    ▼
SpreadFile { workspaces: Vec<Workspace> }  (raw, as written by the user)
    │  config::resolve_paths(file, env, invocation_cwd)
    │  — resolves each workspace's root independently; every workspace gets its
    │    own absolute root, defaulted and ~-expanded exactly as a single-workspace
    │    config would be
    ▼
SpreadFile (every workspace's root defaulted + absolute, ~ expanded everywhere)
    │  engine::apply(&file, &mut backend)
    ▼
for each workspace, in order:
    │  apply_workspace(workspace, &mut backend) → a focus-pane-id candidate
    │  — apply_workspace never calls focus_pane itself; it only returns which
    │    pane *would* be focused if this workspace turns out to be the winner
    ▼
apply keeps a running "chosen" candidate as it goes — a workspace's own
`focus: true` overrides the running choice (last workspace to set it wins),
and the first workspace's candidate is the choice if none does — then, once
every workspace has been built, calls backend.focus_pane(chosen) exactly once
    ▼
an actual set of herdr workspaces, with tabs, panes, commands, and one focused pane
```

Each stage is a separate module and is unit-tested independently; only the final wiring in `main.rs` is untested by design (see [Testing strategy](#testing-strategy)).

## The herdr model: workspace → tab → pane

herdr's own object model is workspace → tab → pane, and the config types mirror it directly: `SpreadFile` holds a list of `Workspace`s (one YAML file can now describe several workspaces, not just one), each `Workspace` has `tabs`, and each tab has `panes` (→ panes within that tab). This is intentionally the same shape as tmuxinator's `windows`/`panes` (herdr calls the same concept a "tab" rather than a "window"), since that's the layout DSL most users porting a config will already know.

The one wrinkle: `herdr workspace create` and `herdr tab create` don't just create an empty tab — they *also* create that tab's first ("root") pane, in the same call. That single fact drives most of the complexity in `engine.rs`, described next.

## `engine.rs`: why "first panes" are special-cased

`engine::apply(&file, backend)` itself only does one thing: it loops over `file.workspaces` in order, calling `apply_workspace(workspace, backend)` for each one, and folds the focus-pane-id candidate each call returns into a single running "chosen" id (see [Focus, deferred across two levels](#focus-deferred-across-two-levels) below). All of the interesting per-layout logic lives in `apply_workspace`, which walks `workspace.tabs[*].panes[*]` and, for each pane, decides which `HerdrBackend` call creates it:

- **A tab's first pane** (`pane_index == 0`) is never created directly — it's the root pane that came back from `create_workspace` (for the first tab) or `create_tab` (for every other tab). There is no `HerdrBackend::create_first_pane` call; it already exists.
- **Every other pane** is created by `split_pane`, splitting off the previous pane in the tab.

This asymmetry matters because `create_workspace`, `create_tab`, and `split_pane` each accept a `cwd`/`env` at creation time — but a first pane, having no creation call of its own, has no way to receive a pane-specific `cwd` or `env` through the API. The fix (after several review rounds got this wrong — see [History of the cwd/env bug](#history-of-the-cwden-bug)) is: when a first pane needs a `cwd` or `env` beyond what its workspace/tab baseline already gives it, `engine.rs` prefixes its `run` command with a shell snippet:

```
cd '<resolved dir>' && export KEY='value' && <the user's command>
```

built by `cwd_env_prefix` / `wrap_command_with_cwd_and_env`, with `shell_quote` doing POSIX single-quote escaping so paths and values with spaces or special characters survive intact. If the first pane has *no* command but does need a `cwd`/`env`, the bare `cd && export` line is still run — otherwise a `cwd:`-only entry in the YAML would be silently ignored. If the first pane needs neither, no `run` call happens at all, avoiding a pointless extra `cd .` (`needs_cwd_override` in `engine.rs` decides this).

Everything else follows directly from that split:

- **Path composition** (`resolve_cwd` / `combine_cwd`) layers `root → tab.cwd → pane.cwd` top-down: each level is joined onto the previous one unless it's already absolute, in which case it replaces everything above it. `..` is deliberately left alone (the shell resolves it at `cd` time); only literal `.` components are stripped (`normalize_path`).
- **`wait_for` on a pane with no `command`** is rejected as `EngineError::WaitForWithoutCommand` — there's no command whose output to wait for.
- **IDs are never invented.** Every `workspace_id`/`tab_id`/`pane_id` used by a later call is one that came back from an earlier `HerdrBackend` response. herdr's own ids get compacted as things are created/closed, so the engine only ever trusts what the backend just told it.

### Focus, deferred across two levels

Focus follows the same design principle it always has — *"focus happens globally exactly once at the very end"* — but a second workspace-per-file level was layered on top of it:

- Every create/split call passes `--no-focus`, so nothing is focused as a side effect of building the layout.
- Within a single workspace, `apply_workspace` tracks whichever pane last had `focus: true` in that workspace's YAML (defaulting to the workspace's very first pane) and *returns* that pane id as its focus candidate. **`apply_workspace` never calls `focus_pane` itself** — it has no way to know yet whether its workspace will be the one that ends up focused.
- Back in `engine::apply`, the outer loop tracks which *workspace* should win, using the exact same "last one wins, default to the first" rule but applied to `workspace.focus` instead of `pane.focus`: the first workspace's candidate is the initial choice, and any later workspace with `focus: true` overwrites it.
- Only after every workspace has been built does `apply` call `backend.focus_pane` — exactly once, for the whole file — with the candidate id belonging to whichever workspace won.

This avoids focus visibly jumping around pane-by-pane (or workspace-by-workspace) while the layout is still being built, and keeps `apply_workspace` fully decoupled from any notion of "am I the focused workspace" — it just answers "if you picked me, focus this pane."

## `config.rs`: path resolution

This is the part of the codebase that took the most iteration to get right, so it's worth understanding as its own subsystem rather than an afterthought of parsing.

`resolve_paths(file, env, invocation_cwd)` runs once, right after loading the YAML and before the engine ever sees the config, and applies the same three rules to *every workspace in the file independently* — a `SpreadFile` with three workspaces resolves three separate `root`s, one per workspace, each against the same `invocation_cwd`:

1. **`root` is defaulted** to `invocation_cwd` if a workspace's YAML doesn't set one, so every workspace has an absolute anchor to resolve relative paths against — a workspace with no `root` and a tab with `cwd: ./logs` still means something well-defined.
2. **`root` is tilde-expanded and forced absolute** (`expand_root_path`): `~` and `~/...` expand against `$HOME`; anything still relative after that is joined onto `invocation_cwd`.
3. **`tab.cwd` and `pane.cwd` are tilde-expanded but *not* forced absolute** (`expand_tilde` only): they're meant to stay relative to their workspace's `root` (that's the whole point of a tab/pane-level override), so only a literal `~` gets special treatment. The actual `root + tab.cwd + pane.cwd` composition happens later, per-pane, inside `apply_workspace`.

### What "invocation cwd" means, and why it isn't `std::env::current_dir()`

The subtle part: when `herdr-spreader` runs as a herdr plugin action, `std::env::current_dir()` returns **the plugin's own install directory**, not the user's project — herdr sets the child process's cwd to wherever the plugin was linked from, not to the workspace or pane that invoked the action. Naively using it as the resolution base would silently root every layout inside `herdr-spreader`'s own checkout.

The fix in `main.rs`: if `HERDR_PANE_ID` is set (i.e. we were invoked by herdr), query that pane's real shell directory via `herdr pane get <id>` (`CliBackend::query_pane_cwd`, reading the `foreground_cwd` field of the response) and use that as the invocation cwd instead. `std::env::current_dir()` is only used as a fallback — for standalone CLI usage outside of any herdr session, where it's correct.

```
HERDR_PANE_ID env var
    │
    ▼
CliBackend::query_pane_cwd  ──▶  `herdr pane get <id>` → foreground_cwd
    │  (None on any failure: no herdr session, pane gone, unexpected response)
    ▼
fallback: std::env::current_dir()
    │
    ▼
config::resolve_paths(..., invocation_cwd)
```

`query_pane_cwd` deliberately collapses every failure mode into `None` rather than propagating an error — a missing or unreachable herdr session is exactly the "standalone CLI" case the fallback exists for, not something worth failing `apply` over.

### History of the cwd/env bug

This subsystem went through six rounds of review, each fixing a real bug the previous round introduced or missed. It's documented here because the same class of mistake is easy to reintroduce:

1. First panes silently dropped `tab.cwd`/`pane.cwd`/`pane.env` entirely (no creation call to pass them to).
2. The fix (a `cd`-prefixed command) broke on `~` and relative `root`, because a single-quoted `cd` can't trigger shell tilde expansion, and a relative root got joined onto itself.
3. `root` got tilde-expanded, but `tab.cwd`/`pane.cwd` had the identical bug.
4. With no `root` at all, relative `tab.cwd`/`pane.cwd` had no absolute base and leaked through unresolved.
5. A guessed JSON schema for reading the invocation directory out of `HERDR_PLUGIN_CONTEXT_JSON` turned out to be unverifiable and likely wrong — replaced by the documented `pane.get`/`foreground_cwd` query described above.
6. (Neutral review, approved) — confirmed the fixes above compose correctly across first panes, split panes, tab 0 vs. tab N, and standalone-vs-plugin invocation.

If you're touching path resolution, add a test for the *specific* combination you changed (root present/absent, tilde/relative/absolute, tab 0 vs. later tabs, first pane vs. split pane) — this area has a track record of looking correct in isolation while being wrong in combination.

## `backend/`: the seam between logic and I/O

`backend/mod.rs` defines `HerdrBackend` — one method per herdr operation the engine needs (`create_workspace`, `create_tab`, `split_pane`, `run`, `wait_output`, `focus_pane`, `rename_tab`), plus the `*Opts`/`*Created` structs passed to and returned from them. This trait is the entire contract between "what layout to build" (`engine.rs`) and "how to actually build it" (`backend/cli.rs`).

`backend/cli.rs` implements that trait by shelling out to the `herdr` binary and parsing its JSON stdout. Internally it's split into two halves on purpose:

- **Pure functions** (`workspace_create_args`, `tab_create_args`, `pane_split_args`, `pane_run_args`, `wait_output_args`, `focus_args`, `rename_tab_args`, `pane_get_args`, and the matching `parse_*` functions) — no I/O, just `Opts → Vec<String>` and `&str (JSON) → Result<T, BackendError>`. These are unit-tested directly, without spawning anything.
- **`CliBackend`** itself — the thin `impl HerdrBackend` that calls those pure functions and then actually runs `std::process::Command`. It's spawned via an argv array (`Command::args`, never a shell string), so there's no shell-injection surface at the herdr-invocation boundary — the only place shell syntax appears is inside the *pane's own command string*, which is sent to that pane's interactive shell via `herdr pane run`, exactly as if the user had typed it themselves.

`CliBackend::resolve_bin` picks the `herdr` binary to spawn: `$HERDR_BIN_PATH` if set (mainly for tests), otherwise `herdr` on `$PATH`.

## Testing strategy

Three layers, each targeting a different seam:

1. **`config.rs` unit tests** — YAML parsing and path resolution, as plain data-in/data-out assertions. No filesystem or process access beyond `load_config`'s own file read.
2. **`engine.rs` unit tests** — drive `apply_workspace` (and `apply`'s cross-workspace focus-selection folding on top of it) against a hand-written `MockBackend` that just records every call it receives (`Vec<Call>`). This is what makes it possible to assert "for this YAML, exactly these `HerdrBackend` calls happen, in this order, with these ids" without needing herdr installed at all.
3. **`tests/cli_backend_integration.rs`** — the one test that exercises the full `engine::apply` → `CliBackend` → subprocess path, against `tests/fixtures/fake-herdr.sh`, a script that logs the argv it's called with and echoes back canned JSON shaped like real herdr responses. This is what catches integration bugs the mock backend can't see (e.g. an argv-building bug in `backend/cli.rs` that both the mock and the real herdr would parse differently).

Layer 3 intentionally does *not* go through `config::resolve_paths` — it builds a `SpreadFile` directly and calls `engine::apply` on it. `main.rs`'s wiring (config-path resolution, the `pane.get` cwd query, `resolve_paths`) is therefore covered only at the unit level, not end-to-end; keep that in mind if a bug ever turns up specifically in how `main.rs` composes those pieces rather than in any one of them.
