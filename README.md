# syld — Support Your Linux Desktop

Discover the open source software you use every day and help you support the projects behind it.

syld scans your system's package managers, identifies the open source projects you rely on, and helps you build a donation plan to give back — even on a small budget.

## Features

- **Package discovery** — reads local package databases directly (no root needed)
- **Privacy-first** — all processing is local by default, no network calls unless you opt in
- **Grouped output** — packages are grouped by upstream project and sorted alphabetically
- **Pagination** — browse results incrementally with `--limit`

### Planned

- **Donation planning** — set a monthly/yearly budget and get a plan to spread it across projects ([#13](https://github.com/bombfork/syld/issues/13))
- **Enrichment (opt-in)** — fetch donation links, bug trackers, and contributing guides from upstream ([#15](https://github.com/bombfork/syld/issues/15))
- **Multiple output formats** — JSON and HTML reports ([#12](https://github.com/bombfork/syld/issues/12))
- **Systemd integration** — user-level timer for periodic scans (unit files ship in `systemd/`)

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
syld scan --limit 50    # show more results (0 for all)
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
