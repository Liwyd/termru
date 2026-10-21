# Installing Termru

One command per operating system. Nothing has to be compiled, and nothing
is piped into a shell.

The packages are published on the [release page](https://github.com/Liwyd/termru/releases).
Nothing is signed: the trust anchor is the `SHA256SUMS` file next to the
assets (see [Verifying the download](#verifying-the-download)).

Set the version you want once:

```bash
VERSION=0.1.0
BASE="https://github.com/Liwyd/termru/releases/download/v${VERSION}"
```

## Debian / Ubuntu (`.deb`)

```bash
VERSION=0.1.0
curl -fLO "https://github.com/Liwyd/termru/releases/download/v${VERSION}/termru_${VERSION}_amd64.deb"
sudo apt install ./termru_${VERSION}_amd64.deb
```

`apt` pulls in the runtime dependencies (WebKitGTK, GTK 3, the app
indicator). The application is installed as `/usr/bin/termru` and starts
from the desktop menu.

To install it for the current user only, without `sudo`:

```bash
sudo apt install --root=$HOME/.local ./termru_${VERSION}_amd64.deb
```

## Fedora / RHEL / openSUSE (`.rpm`)

```bash
VERSION=0.1.0
curl -fLO "https://github.com/Liwyd/termru/releases/download/v${VERSION}/termru-${VERSION}-1.x86_64.rpm"
sudo dnf install ./termru-${VERSION}-1.x86_64.rpm
```

## Any Linux (AppImage)

No installation, no root rights:

```bash
VERSION=0.1.0
curl -fLO "https://github.com/Liwyd/termru/releases/download/v${VERSION}/termru_${VERSION}_amd64.AppImage"
chmod +x termru_${VERSION}_amd64.AppImage
./termru_${VERSION}_amd64.AppImage
```

The AppImage needs `FUSE` to mount itself. On a distribution without it:

```bash
./termru_${VERSION}_amd64.AppImage --appimage-extract-and-run
```

Put it in `~/Applications` (or wherever you keep your portable programs)
and add it to the menu of your desktop environment by hand.

## Windows (NSIS installer)

The installer installs for the current user; no administrator rights are
needed.

1. Download `termru_${VERSION}_x64-setup.exe` from the release page.
2. Run it.

The installer is not signed, so Windows SmartScreen will warn about an
unknown publisher. "More info" → "Run anyway" is the expected path for an
unsigned application.

## Verifying the download

Every release carries a `SHA256SUMS` file with the checksum of every
asset:

```bash
curl -fLO "https://github.com/Liwyd/termru/releases/download/v${VERSION}/SHA256SUMS"
sha256sum --check --ignore-missing SHA256SUMS
```

The output must list every file you downloaded with `OK`. A mismatch means
the download is broken — do not run the file.

## Where the data lives

| What | Where |
|---|---|
| The database (hosts, groups, tags, preferences) | `~/.local/share/com.liwyd.termru/termru.db` (Linux), `%APPDATA%\com.liwyd.termru\` (Windows) |
| The log files | `~/.local/share/com.liwyd.termru/logs/` |
| The secrets (passwords, passphrases, sync tokens) | the credential store of the desktop environment (GNOME Keyring, KWallet, the Windows Credential Manager) — never in the database |

See [backup and restore](../docs/development.md#the-data-and-the-backup) for
how to back the database up.

## Uninstalling

```bash
sudo apt remove termru     # Debian / Ubuntu
sudo dnf remove termru     # Fedora
```

The data directory stays; remove it by hand if you want a clean slate:

```bash
rm -rf ~/.local/share/com.liwyd.termru
```

## The cloud server (optional)

Termru works without a server. If you want your host configuration on
more than one device, run the self-hosted server — see
[the deployment guide](cloud/deployment.md):

```bash
curl -fsSLO "https://github.com/Liwyd/termru/releases/download/v${VERSION}/compose.yaml"
curl -fsSLO "https://github.com/Liwyd/termru/releases/download/v${VERSION}/compose.yaml.sha256"
sha256sum --check compose.yaml.sha256
docker compose up -d
```
