[![CI][ci-badge]][ci] [![Release (x86_64)][rel-x86-badge]][rel-x86] [![Release (aarch64)][rel-aarch64-badge]][rel-aarch64]

[ci-badge]: https://github.com/bombfork/syld/actions/workflows/ci.yml/badge.svg
[ci]: https://github.com/bombfork/syld/actions/workflows/ci.yml
[rel-x86-badge]: https://github.com/bombfork/syld/actions/workflows/release-x86_64.yml/badge.svg
[rel-x86]: https://github.com/bombfork/syld/actions/workflows/release-x86_64.yml
[rel-aarch64-badge]: https://github.com/bombfork/syld/actions/workflows/release-aarch64.yml/badge.svg
[rel-aarch64]: https://github.com/bombfork/syld/actions/workflows/release-aarch64.yml

# syld — Support Your Linux Desktop

<div align="center">

<img src="https://imgs.xkcd.com/comics/dependency.png" alt="xkcd 2347: Dependency — All modern digital infrastructure held up by a project some random person in Nebraska has been mass maintaining since 2003" width="400">

<em><a href="https://xkcd.com/2347/">xkcd #2347</a></em>

</div>

&nbsp;

Your desktop runs on the quiet work of thousands of open source maintainers — people you've never met, probably in places you've never been. Some random person in Nebraska has been mass maintaining a project since 2003, and your entire digital life might depend on it. You don't know them, but you owe them.

**syld** is here to help you give that person some love.

It scans your system's package managers, figures out which open source projects you actually rely on every day, and helps you find ways to contribute back — whether that's code, documentation, bug reports, or just a thank you.

## Features

- **Package discovery** — reads your local package databases directly (no root needed)
- **Privacy-first** — everything stays on your machine by default, no network calls unless you opt in
- **Grouped output** — packages are grouped by upstream project so you can see who's behind what
- **Systemd integration** — set-and-forget periodic scans with a user-level timer (`syld install service`)
- **Hooks** — integration with package manager to be reminded on system updates about useful way to contribute (`syld install hooks`)

### Supported package managers

pacman, apt, dnf, Flatpak, Snap, Nix, mise, Homebrew/Linuxbrew, Docker, Podman

## Installation

### From source

Requires [Rust](https://rustup.rs/) (2024 edition) or [mise](https://mise.jdx.dev/):

```sh
git clone https://github.com/bombfork/syld.git
cd syld
cargo build --release
cp target/release/syld ~/.local/bin/
```

### First-time setup

The recommended way to get started is the interactive setup wizard:

```sh
syld setup
```

This walks you through configuration, installs the systemd timer and package manager hooks, and runs an initial scan — making syld fully operational in one command.

### Manual installation (alternative)

If you prefer non-interactive or scripted installs, use the individual install commands:

```sh
syld install service --frequency weekly --enable   # systemd user timer
syld install hook pacman-post-transaction           # pacman ALPM hook (requires sudo)
```

## Usage

syld follows a three-step workflow: **setup → discover → review**.

### 1. Setup

```sh
syld setup                         # interactive first-run wizard
syld config set enrich true        # opt in to network enrichment
syld config show                   # view current settings
syld config edit                   # open config in $EDITOR
```

| Key | Type | Description |
|-----|------|-------------|
| `enrich` | bool | Enable network enrichment by default |
| `enrich_jobs` | number | Parallel enrichment threads (default: 4) |

### 2. Discover

```sh
syld                        # scan installed packages (default action)
syld scan                   # same as above
syld scan --limit 50        # show more results (0 for all)
```

### 3. Review

```sh
syld report                 # terminal report from last scan (enriches if configured)
syld report --force-refresh # re-fetch enrichment data, bypassing cache
syld report --format json   # machine-readable output
syld report --format html   # HTML report
```

### 4. Hooks

Package manager hooks surface contribution opportunities after transactions. A prior `syld scan` + `syld report` (with `enrich = true` in config) is needed for the hook to have data.

```sh
syld hook list                      # show available hooks
syld hook run pacman-post-transaction  # run a hook manually (reads stdin)
syld install hook pacman-post-transaction  # install a hook
```

## Configuration

syld follows the [XDG Base Directory Specification](https://specifications.freedesktop.org/basedir-spec/latest/):

| Path | Purpose |
|------|---------|
| `~/.config/syld/config.toml` | User configuration |
| `~/.local/share/syld/` | Scan history and data |
| `~/.cache/syld/` | Enrichment cache |

Example `config.toml`:

```toml
enrich = false
```

## Development

This project uses [mise](https://mise.jdx.dev/) for tool management and tasks.

```sh
mise install          # install toolchain
mise run check        # run all checks (fmt, lint, build, test)
mise run fmt          # auto-format
mise run lint         # auto-fix clippy warnings
```

## Privacy

syld respects your privacy:

- **Default mode**: reads only local package databases. Zero network access.
- **Enriched mode** (`enrich = true` in config): opt-in only. Fetches project metadata from public sources (GitHub, GitLab, Open Collective, Liberapay). No personal data is sent.
- No telemetry, no tracking, no accounts.

## License

[GPL-3.0-or-later](LICENSE)
