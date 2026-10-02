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

1. Open the `Sirvo Hub_x.y.z_aarch64.dmg` and drag **Sirvo Hub.app** into
   `/Applications`.
2. Because the app is not yet notarized, macOS Sequoia (15+) shows a "Sirvo
   Hub is damaged and can't be opened" dialog on first launch. The app is
   fine — that's Apple's hardened Gatekeeper response to unsigned apps with
   a browser-applied quarantine flag. The old right-click → Open bypass no
   longer works in Sequoia. Strip the quarantine flag from Terminal:

   ```bash
   xattr -dr com.apple.quarantine "/Applications/Sirvo Hub.app"
   open "/Applications/Sirvo Hub.app"
   ```

   You only need to run this once; subsequent launches work normally.
3. Sirvo Hub appears in the menu-bar tray.

Signing/notarization is deferred — v1 ships unsigned. The auto-update channel
will deliver a signed build transparently once available.

### Connect it to Odoo

The hub doesn't start until it has an Odoo connection. On first launch the
**Connection to Odoo** window opens; it is also in the tray menu.

1. In Odoo, create a user for this hub only (for example `hub-barilo`):
   - give it a long random password, not `admin`, which the hub refuses;
   - give it the POS rights;
   - add it to **POS Hub Service**, so terminals can still log in while Odoo
     can't be reached.
2. In the window, enter the Odoo URL, database, that user and its password,
   then click **Test and save**.
   - If Odoo is behind a WAF that requires an `x-api-token` header (it answers
     403 without it), also fill in **WAF token**. The tray sends it on the
     test, and the hub sends it on every request to Odoo (`ODOO_RPC_HEADERS`;
     needs a bundled hub that includes rost_pos_restaurant#172 or later).
   - Nothing is saved unless Odoo accepts the login.
   - The hub then restarts with the new settings.

**Where the settings are kept:**
- URL, database and user: `settings.json`, in the app config folder.
- Password and WAF token: the OS credential store (Windows Credential Manager, macOS
  Keychain, Linux Secret Service). Where that isn't available, it goes in an
  `odoo-secret` file that only this OS user can read, and the window says so.

**To cut off a hub,** archive its user in Odoo.

If the hub refuses its settings (exit code 78), the tray shows "Odoo settings
refused" and reopens the window with the hub's reason.

See [docs/spec_odoo_credentials.md](docs/spec_odoo_credentials.md).

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
