# syld — Support Your Linux Desktop

Discover the open source software you use every day and help you support the projects behind it.

syld scans your system's package managers, identifies the open source projects you rely on, and helps you build a donation plan to give back — even on a small budget.

## Features

- **Package discovery** — reads local package databases directly (no root needed)
- **Privacy-first** — all processing is local by default, no network calls unless you opt in
- **Donation planning** — set a monthly/yearly budget and get a plan to spread it across projects
- **Enrichment (opt-in)** — fetch donation links, bug trackers, and contributing guides from upstream
- **Multiple output formats** — terminal, JSON, HTML
- **Systemd integration** — ship with a user-level timer for periodic scans

### Supported package managers

| Package manager | Status |
|-----------------|--------|
| pacman (Arch)   | Working |
| apt (Debian/Ubuntu) | Planned |
| dnf (Fedora/RHEL) | Planned |
| Flatpak         | Planned |
| Snap            | Planned |
| Nix             | Planned |

## Installation

### From source

Requires [Rust](https://rustup.rs/) (2024 edition) or [mise](https://mise.jdx.dev/):

```sh
git clone https://github.com/bombfork/syld.git
cd syld
cargo build --release
cp target/release/syld ~/.local/bin/
```

### Systemd timer (optional)

To run syld weekly as a user service:

```sh
cp systemd/syld.service systemd/syld.timer ~/.config/systemd/user/
systemctl --user enable --now syld.timer
```

## Usage

```sh
# Scan installed packages (default action)
syld
syld scan

# Generate reports
syld report
syld report --format json
syld report --enrich          # fetch donation links (network access)

# Manage your support budget
syld budget set 5 --cadence monthly
syld budget plan
syld budget plan --strategy weighted
syld budget show

# Configuration
syld config show
syld config edit
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

This project uses [mise](https://mise.jdx.dev/) for tool management and tasks, and [hk](https://hk.jdx.dev/) for git hooks.

```sh
mise install          # install toolchain
mise run check        # run all checks (fmt, lint, build, test)
mise run fmt          # auto-format
mise run lint         # auto-fix clippy warnings
```

Pre-commit hooks (format + lint) are enforced via hk:

```sh
hk install --mise     # set up git hooks
```

## Privacy

syld respects your privacy:

- **Default mode**: reads only local package databases. Zero network access.
- **Enriched mode** (`--enrich`): opt-in only. Fetches project metadata from public sources (GitHub, GitLab, Open Collective, Liberapay). No personal data is sent.
- No telemetry, no tracking, no accounts.

## License

[GPL-3.0-or-later](LICENSE)
