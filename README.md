# syld — Support Your Linux Desktop

Discover the open source software you use every day and help you support the projects behind it.

syld scans your system's package managers, identifies the open source projects you rely on, and helps you build a donation plan to give back — even on a small budget.

## Features

- **Package discovery** — reads local package databases directly (no root needed)
- **Privacy-first** — all processing is local by default, no network calls unless you opt in
- **Grouped output** — packages are grouped by upstream project and sorted alphabetically
- **Pagination** — browse results incrementally with `--limit`

- **Systemd integration** — user-level timer for periodic scans (`syld install service`)

### Supported package managers

| Package manager | Status |
|-----------------|--------|
| pacman (Arch)   | Working |
| apt (Debian/Ubuntu) | Planned ([#1](https://github.com/bombfork/syld/issues/1)) |
| dnf (Fedora/RHEL) | Planned ([#2](https://github.com/bombfork/syld/issues/2)) |
| Flatpak         | Planned ([#3](https://github.com/bombfork/syld/issues/3)) |
| Snap            | Planned ([#4](https://github.com/bombfork/syld/issues/4)) |
| Nix             | Planned ([#5](https://github.com/bombfork/syld/issues/5)) |
| mise            | Planned ([#6](https://github.com/bombfork/syld/issues/6)) |
| Homebrew/Linuxbrew | Planned ([#7](https://github.com/bombfork/syld/issues/7)) |

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

Or copy the reference template files directly:

```sh
cp systemd/syld.service systemd/syld.timer ~/.config/systemd/user/
systemctl --user enable --now syld.timer
sudo cp hooks/pacman/syld.hook /usr/share/libalpm/hooks/
```

Note: the template files hardcode default binary paths. The `syld install` commands generate files with the correct path to your syld binary.

## Usage

syld follows a four-step workflow: **setup → discover → review → budget**.

### 1. Setup

```sh
syld setup                         # interactive first-run wizard
syld config set enrich true        # opt in to network enrichment
syld config set budget.currency EUR
syld config show                   # view current settings
syld config edit                   # open config in $EDITOR
```

| Key | Type | Description |
|-----|------|-------------|
| `enrich` | bool | Enable network enrichment by default |
| `enrich_jobs` | number | Parallel enrichment threads (default: 4) |
| `budget.amount` | number | Support budget amount |
| `budget.currency` | string | Currency code (default: USD) |
| `budget.cadence` | string | `monthly` or `yearly` (default: monthly) |

### 2. Discover

```sh
syld                        # scan installed packages (default action)
syld scan                   # same as above
syld scan --limit 50        # show more results (0 for all)
```

### 3. Review

```sh
syld report                 # terminal report from last scan
syld report --enrich        # fetch donation links, stars, etc.
syld report --format json   # machine-readable output
syld report --format html   # HTML report
```

### 4. Budget

```sh
syld budget set 10                  # set a monthly budget
syld budget set 120 --cadence yearly
syld budget show                    # display current budget
syld budget plan                    # generate a donation plan
syld budget plan --strategy weighted
```

### 5. Hooks

Package manager hooks surface contribution opportunities after transactions. A prior `syld scan` + `syld report --enrich` is needed for the hook to have data.

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
| `~/.local/share/syld/` | Scan history and budget data |
| `~/.cache/syld/` | Enrichment cache |

Example `config.toml`:

```toml
enrich = false

[budget]
amount = 5.0
currency = "EUR"
cadence = "monthly"
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
- **Enriched mode** (`--enrich`): opt-in only. Fetches project metadata from public sources (GitHub, GitLab, Open Collective, Liberapay). No personal data is sent.
- No telemetry, no tracking, no accounts.

## License

[GPL-3.0-or-later](LICENSE)
