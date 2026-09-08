# Installation

qfs is a single binary. There's nothing to configure to get started — you can describe compiled
paths and preview writes against built-in routed paths with no credentials at all.

## Install script (recommended)

```sh
curl -fsSL https://raw.githubusercontent.com/qmu/qfs/main/packages/qfs/install.sh | sh
```

The script detects your OS and architecture, downloads the matching release, **verifies its
checksum before extracting**, and installs `qfs` to `~/.local/bin`. It never asks for or fetches
any credential.

::: tip Requires a published release
The script downloads from the project's GitHub Releases. If no release has been published yet,
[build from source](#build-from-source) instead — it's one command. (Releases are cut by pushing a
`v*` tag, which builds the Linux and macOS binaries in CI and attaches them.)
:::

To install somewhere else:

```sh
curl -fsSL https://raw.githubusercontent.com/qmu/qfs/main/packages/qfs/install.sh | QFS_INSTALL_DIR=/usr/local/bin sh
```

Make sure the install directory is on your `PATH`. Then check it works:

```sh
qfs --version
```

## Build from source

You'll need a recent Rust toolchain. The qfs workspace lives under `packages/qfs/`:

```sh
git clone https://github.com/qmu/qfs
cd qfs/packages/qfs
cargo build --release        # produces target/release/qfs
./target/release/qfs --version
```

## Supported platforms

Released binaries are built for:

- **Linux** — `x86_64` and `aarch64` (static musl, no system dependencies)
- **macOS** — `x86_64` (Intel) and `aarch64` (Apple Silicon)

## First check (no credentials needed)

You can immediately explore the language without connecting anything. `describe` is offline, and a
write preview against the built-in `/sys` mount performs no I/O:

```sh
# What can I do with a mail draft?
qfs describe /mail/drafts

# Preview a routed write — shows the plan, changes nothing
qfs run "insert into /sys/policies values (name, allow) ('first-preview', 'ALLOW INSERT')"
```

An unconnected cloud write such as `/mail/drafts` refuses with `unrouted_path`; preview is safe, but
it does not bypass routing setup. When you're ready to use a real service, [connect it](/guide/connect).

## Use qfs from Codex (the plugin)

The QFS plugin packages the same skills for Codex and Claude Code. Install the `qfs` binary first;
the plugin teaches the CLI and does not bundle service credentials or an MCP server.

For a checkout of this repository, run these commands from the repository root:

```sh
codex plugin marketplace add .
codex plugin add qfs@qfs
codex plugin list --json --marketplace qfs
```

These commands were verified with Codex CLI `0.153.4`: the listing reported `qfs@qfs` installed
and enabled from this checkout, with both `qfs:qfs` and `qfs:qfs-slack` available to a new thread.

The local marketplace is `.agents/plugins/marketplace.json`, pointing to `plugins/qfs` and its
`.codex-plugin/plugin.json`. Check that the listing reports QFS installed and enabled, and that
the marketplace root is the checkout you intend to use. To install from GitHub instead, use
`codex plugin marketplace add qmu/qfs` before `codex plugin add qfs@qfs`; a remote snapshot does not
pick up edits made to a local checkout.

After updating a local plugin's version, run `codex plugin add qfs@qfs` again. Start a new Codex
thread to verify automatic discovery: ask it to use QFS to find a channel by name **without giving
an ID**, and confirm it reads the connection and workspace `describe` output before choosing a
channel collection. The plugin's registered skill names are `qfs:qfs` and `qfs:qfs-slack`;
select them from Codex's skill picker (or explicitly invoke `$qfs:qfs` / `$qfs:qfs-slack`). Installation and
cache freshness do not prove that an already-running thread has refreshed its skill catalog;
reading updated skill files into that thread explicitly is a separate operation.

The repository's packaging check is `python3 scripts/check-plugin.py`, with negative fixtures in
`python3 scripts/test-check-plugin.py`. It checks both host manifests, exact registrations and
Cookbook-generated content; `cargo run -p xtask -- gen-skills --check` from `packages/qfs` remains
the generator-owned backstop. These checks verify distribution, not live agent behavior.

See the [official plugin guide](https://developers.openai.com/plugins/build/plugins) for Codex's
marketplace and plugin packaging model.

## Use qfs from Claude Code (the plugin)

qfs ships a **Claude Code plugin** that bundles the qfs *skills* — the describe→preview→commit how-to
an agent loads on demand — so Claude can drive the `qfs` CLI over your connected services. Add the
marketplace and install it:

```
/plugin marketplace add qmu/qfs
/plugin install qfs@qfs
```

This lands in `~/.claude/settings.json`:

```jsonc
{
  "extraKnownMarketplaces": {
    "qfs": { "source": { "source": "github", "repo": "qmu/qfs" } }
  },
  "enabledPlugins": {
    "qfs@qfs": true
  }
}
```

The plugin carries knowledge, not credentials: its skills shell out to the `qfs` binary you
installed above, so finish a one-time [connect setup](/guide/connect) for the services you
want. The agent inherits qfs's safety model unchanged — every write previews first, and irreversible
actions (sending a draft, trashing, merging a PR) still need an explicit `--commit-irreversible`, so
an agent can't fire them by accident.

**Next:** [Get started →](/guide/getting-started)
