# Hub Tray App

A cross-platform (Windows + macOS) Tauri tray app that supervises a bundled copy
of `nu_pos_hub`. It starts the hub in the background on login, shows current
status (running terminals, LAN URL) in the tray menu, exposes a logs window,
and auto-updates silently from GitHub Releases. EN/ES UI.

## Install (restaurant)

Download the latest release for your platform from the releases page:

https://github.com/josespinal/sirvo-hub-tray/releases

### Windows

1. Run the `Sirvo-Hub-Setup-x.y.z.exe` installer.
2. Because the binary is not yet code-signed, Windows SmartScreen will warn
   you with "Windows protected your PC". Click **More info**, then
   **Run anyway**.
3. Sirvo Hub will start automatically and appear in the system tray.

### macOS

1. Open the `Sirvo-Hub_x.y.z_universal.dmg` and drag **Sirvo Hub.app** into
   `/Applications`.
2. Because the app is not yet notarized, Gatekeeper will refuse to open it on
   first launch. Right-click **Sirvo Hub.app**, choose **Open**, then click
   **Open** in the confirmation dialog. Subsequent launches work normally.
3. Sirvo Hub appears in the menu-bar tray.

Signing/notarization is deferred — v1 ships unsigned. The auto-update channel
will deliver a signed build transparently once available.

## Uninstall

### Windows

Settings → **Apps** → **Sirvo Hub** → **Uninstall**.

### macOS

Drag `/Applications/Sirvo Hub.app` to the Trash. For a clean removal also
delete `~/Library/Application Support/com.sirvo.hub.tray`.

## Data locations

The bundled hub and the tray app share Tauri's per-platform data dirs:

| Platform | SQLite DB | Logs |
| --- | --- | --- |
| macOS | `~/Library/Application Support/com.sirvo.hub.tray/hub.sqlite` | `~/Library/Logs/com.sirvo.hub.tray/hub.log` |
| Windows | `%APPDATA%\com.sirvo.hub.tray\hub.sqlite` | `%APPDATA%\com.sirvo.hub.tray\logs\hub.log` |

Paths are derived from Tauri's `app_data_dir` and `app_log_dir`.

## Dev quickstart

```bash
git clone --recurse-submodules git@github.com:josespinal/sirvo-hub-tray.git
cd sirvo-hub-tray
npm install
npm run tauri:dev
```

If you cloned without `--recurse-submodules`, run `git submodule update --init`
first — the hub source lives in `external/source/` (see below).

`tauri:dev` runs `prepare:all` automatically, which stages the per-platform
Node runtime and builds the `nu_pos_hub` bundle into the Tauri resources
directory.

## Source repo

`nu_pos_hub` and `nu_pos_sync_protocol` (the supervised hub and its protocol)
live in the upstream monorepo and are pulled into this repo as a git submodule
at `external/source/`:

- Monorepo: https://github.com/josespinal/rost_pos_restaurant
- Hub: `external/source/nu_pos_hub/`
- Sync protocol: `external/source/nu_pos_sync_protocol/`

To pull the latest hub changes locally:

```bash
cd external/source && git fetch && git checkout main && git pull
cd ../.. && git add external/source && git commit -m "Bump hub source"
```

## Architecture

A Rust Tauri shell (`src-tauri/`) supervises the bundled Node runtime running
`nu_pos_hub`'s built bundle, polling an admin status endpoint and surfacing
state through tray icons/menu, a logs window, and notifications. A small
React webview (`src/`) renders the logs and settings screens. See
[`docs/plan.md`](docs/plan.md) for the full design.
