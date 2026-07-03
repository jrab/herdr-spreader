# herdr-spreader

**Spin up your whole [herdr](https://herdr.dev) workspace layout — tabs, panes, commands, and all — from a single YAML file.**

If you've used [tmuxinator](https://github.com/tmuxinator/tmuxinator) or [tmuxp](https://github.com/tmux-python/tmuxp) with tmux, this is the same idea for herdr: describe the shape of your project's workspace once, and reproduce it with one command instead of manually splitting panes and typing the same commands every time you sit down to work.

```yaml
workspaces:
  - name: frontend
    root: ~/code/my-project/frontend
    env:
      NODE_ENV: development
    tabs:
      - label: editor
        cwd: ./src
        panes:
          - command: nvim
          - split: down
            ratio: 0.3
            command: npm run dev
  - name: backend
    root: ~/code/my-project/backend
    focus: true
    env:
      RUST_LOG: debug
    tabs:
      - label: editor
        cwd: ./src
        panes:
          - command: nvim
            focus: true
          - split: down
            ratio: 0.3
            command: cargo watch -x test
            wait_for:
              match: "Compiling"
              timeout_ms: 10000
      - label: server
        panes:
          - command: cargo run
```

```
$ herdr-spreader apply
```

...and you get two fully laid-out workspaces: `frontend`, with an `editor` tab running your dev server, and `backend`, with an `editor` tab (your editor plus a live test watcher split underneath it) and a `server` tab running `cargo run` — with focus landing in `backend`, on the pane you marked `focus: true`, once every workspace in the file has been built.

## Features

- **Declarative YAML layouts** — describe tabs and panes once, apply them as many times as you want.
- **Nested pane splits** — split panes `right` or `down` with an optional `ratio`, chained from the previous pane, so you can build arbitrarily deep layouts.
- **Per-pane and per-tab working directories** — set a `root` for the whole layout and override it per tab or per pane; relative paths resolve against their parent, `~` expands to your home directory.
- **Environment variables at every level** — set env vars for the whole workspace or scope them to a single pane.
- **Startup commands with synchronization** — run a command in each pane, and optionally `wait_for` a pattern in its output (with a timeout) before moving on — handy for "don't run the tests until the dev server says it's ready."
- **Explicit focus control** — mark exactly which pane should end up focused after the layout is built.
- **Runs as a herdr plugin or a standalone CLI** — invoke it from herdr's plugin menu, or run the binary directly against any config file.
- **Strict config validation** — unknown YAML keys are rejected at parse time instead of being silently ignored, so typos in your config surface immediately.

## Installation

### As a herdr plugin (recommended)

```bash
git clone https://github.com/yuk1ty/herdr-spreader.git
herdr plugin link ./herdr-spreader
```

This builds the release binary and registers the plugin with herdr. From then on, invoke it from within any herdr workspace:

```bash
herdr plugin action invoke herdr-spreader.apply
```

or trigger it from herdr's action menu (`Apply layout`).

### As a standalone CLI

```bash
cargo build --release
./target/release/herdr-spreader apply --file ./spread.yml
```

This works outside of a herdr session too, as long as a herdr server is already running (`herdr server` or `brew services start herdr`) and the `herdr` binary is on your `PATH`.

## Usage

```bash
herdr-spreader apply [--file <path>]
```

| Flag | Description |
|---|---|
| `-f, --file <path>` | Path to a layout YAML file. If omitted, resolved in order: `HERDR_PLUGIN_CONFIG_DIR/spread.yml` (when run as a plugin) → `~/.config/herdr-spreader/spread.yml`. |

## Configuration reference

A layout file has four levels: the **file** (top level), **workspaces**, **tabs**, and **panes** (splits within a tab) — mirroring herdr's own workspace → tab → pane model, with the file itself holding a list of workspaces so one YAML file can describe more than one.

### File

| Key | Type | Description |
|---|---|---|
| `workspaces` | list of [Workspace](#workspace) (required) | Workspaces to create, in order. |

### Workspace

| Key | Type | Description |
|---|---|---|
| `name` | string (required) | Label for the created workspace. |
| `root` | path | Base working directory for this workspace. Supports `~`. If omitted, defaults to the directory you invoked `herdr-spreader` from (or, when run as a plugin, the workspace/pane you invoked it in — not the plugin's own install directory). |
| `env` | map of string→string | Environment variables applied to the workspace's root pane. |
| `tabs` | list of [Tab](#tab) | Tabs to create, in order. |
| `focus` | boolean | Whether the layout's final focus should land in this workspace, once every workspace in the file has been built. Default: `false`. If no workspace sets `focus: true`, the first workspace is used. If multiple workspaces set `focus: true`, the last one wins. |

### Tab

| Key | Type | Description |
|---|---|---|
| `label` | string | Tab name. The first tab renames herdr's default tab instead of creating a new one. |
| `cwd` | path | Working directory for this tab's panes, relative to `root` unless it starts with `~` or `/`. |
| `panes` | list of [Pane](#pane) | Panes to create in this tab, in order. The first pane reuses the tab's root pane; every subsequent pane is created by splitting the previous one. |

### Pane

| Key | Type | Description |
|---|---|---|
| `command` | string | Shell command to run in this pane once it's created. |
| `cwd` | path | Working directory for this pane, relative to the tab's `cwd` (and, transitively, `root`) unless it starts with `~` or `/`. |
| `env` | map of string→string | Environment variables scoped to this pane. |
| `split` | `right` \| `down` | Direction to split from the previous pane. Ignored for a tab's first pane. Default: `right`. |
| `ratio` | float | Size ratio for the split (e.g. `0.3` gives the new pane 30% of the space). |
| `wait_for.match` | string | Substring to wait for in the pane's output after running `command`, before moving on to the next pane. |
| `wait_for.timeout_ms` | integer | How long to wait for the match, in milliseconds. |
| `focus` | boolean | Mark this pane as the focus candidate for its workspace. If no pane in the workspace sets `focus: true`, the workspace's first pane is its candidate. Only the candidate belonging to the workspace that wins the file's top-level `focus` (see [Workspace](#workspace) above) is actually focused, once every workspace in the file has been built. |

Setting `wait_for` on a pane with no `command` is a configuration error and `apply` will fail — there's nothing to wait for output from.

### Path resolution

Paths compose top-down: `root` → tab `cwd` → pane `cwd`, each relative override layered on top of the last, with `..` left for the shell to resolve and `~` expanded against your home directory at every level. An absolute path at any level replaces everything above it.

## How it works

`herdr-spreader` doesn't call any private herdr API — it drives the same `herdr` CLI you'd use by hand, repeating steps 1-5 below for each workspace in the file, in order:

1. `herdr workspace create` — creates the workspace and its first tab/pane.
2. For each subsequent tab, `herdr tab create` — creates a new tab.
3. For each pane after the first in a tab, `herdr pane split` — splits off a new pane in the requested direction.
4. `herdr pane run` — runs the configured command in each pane. A tab's or workspace's first pane can't be created with a working directory or environment variables the way split panes can, so `herdr-spreader` prefixes the command with `cd <dir> && export KEY=VAL && ...` for those panes.
5. `herdr wait output` — for panes with `wait_for`, blocks until the pattern appears before continuing.

Only once every workspace in the file has been built does `herdr-spreader` call `herdr pane focus` — exactly once for the whole file, never once per workspace — landing on the focus candidate (see [Configuration reference](#configuration-reference)) belonging to whichever workspace won: its own `focus: true`, the first workspace by default, or the last workspace to claim `focus: true` if more than one did.

Everything is threaded through the IDs each herdr command actually returns — nothing is guessed or hardcoded — so the layout is built correctly however herdr happens to number workspaces, tabs, and panes at runtime.

## Development

See [ARCHITECTURE.md](./ARCHITECTURE.md) for how the codebase is put together — module boundaries, data flow, and the reasoning behind the trickier parts (path resolution in particular).

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt

# Iterate on the plugin against a real herdr instance
herdr plugin link ./
herdr plugin action invoke herdr-spreader.apply
```

The test suite covers YAML parsing, layout expansion (against a mock backend), and CLI argument/response handling, plus one integration test that drives the full pipeline against a fake `herdr` binary fixture.

## License

[MIT](./LICENSE)
