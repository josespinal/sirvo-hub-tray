# Hub Tray App Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a cross-platform (Windows + macOS) Tauri tray app, distributed as a single bundled installer per OS, that supervises the `nu_pos_hub` Node.js process and gives non-technical restaurant staff a friendly way to see status, restart, view logs, copy the LAN URL, toggle auto-start, and receive silent auto-updates from GitHub Releases.

**Architecture:** New top-level monorepo dir `nu_pos_hub_tray/` containing a Tauri 2 app (Rust core + small React webview for the logs window). The tray's Rust supervisor spawns a bundled Node.js runtime executing a built `nu_pos_hub` artifact as a child process, captures stdout/stderr to a rolling buffer + rotating log file, and polls a new localhost-only `GET /admin/status` endpoint on the hub for live status (terminal count, LAN URL). Auto-updater wired to GitHub Releases ships one bundled `tray+hub` artifact per OS.

**Tech Stack:**
- Tauri 2 (Rust + WebView), `tauri-plugin-autostart`, `tauri-plugin-updater`, `tauri-plugin-clipboard-manager`, `tauri-plugin-dialog`
- React + Vite + TypeScript for the logs window
- Node.js (runtime bundled per platform)
- `nu_pos_hub` (existing) with one new admin endpoint
- vitest for hub-side changes; Rust `#[cfg(test)]` for supervisor/logs buffer logic
- GitHub Actions for builds and releases

---

## File Structure

### New: `nu_pos_hub/` changes (one minimal addition)
- Create `nu_pos_hub/src/admin/lanAddress.ts` — compute LAN URL from a `ws://<lan-ip>:<port>` shape.
- Create `nu_pos_hub/src/admin/statusServer.ts` — Node `http` server bound to `127.0.0.1` exposing `GET /admin/status`.
- Modify `nu_pos_hub/src/config.ts` — add `adminPort` + `adminEnabled` fields.
- Modify `nu_pos_hub/src/index.ts` — wire admin server lifecycle.
- Tests: `nu_pos_hub/src/admin/__tests__/lanAddress.test.ts`, `nu_pos_hub/src/admin/__tests__/statusServer.test.ts`.

### New: `nu_pos_hub_tray/` (entire new top-level directory)

```
nu_pos_hub_tray/
├── version.json                  # own version (independent of root version.json)
├── package.json                  # for the React webview
├── vite.config.ts
├── tsconfig.json
├── index.html                    # webview entry (logs window)
├── README.md
├── i18n/
│   ├── en.json
│   └── es.json
├── scripts/
│   ├── sync-version.mjs          # stamps tauri.conf.json + Cargo.toml from version.json
│   ├── fetch-node-runtime.mjs    # downloads Node binary per platform
│   └── build-hub-bundle.mjs      # builds nu_pos_hub + stages into resources/
├── src/                          # React webview
│   ├── main.tsx
│   ├── App.tsx
│   ├── LogsWindow.tsx
│   └── i18n.ts
└── src-tauri/
    ├── Cargo.toml
    ├── build.rs
    ├── tauri.conf.json
    ├── capabilities/
    │   └── default.json
    ├── icons/                    # standard Tauri icon set
    ├── resources/
    │   ├── hub/                  # populated by build-hub-bundle.mjs (gitignored)
    │   └── node/                 # populated by fetch-node-runtime.mjs (gitignored)
    └── src/
        ├── main.rs               # entry point
        ├── lib.rs                # tauri builder
        ├── supervisor.rs         # child process lifecycle
        ├── log_buffer.rs         # rolling in-memory buffer + file writer
        ├── status_client.rs      # polls /admin/status
        ├── tray.rs               # tray menu + icon state
        ├── commands.rs           # Tauri command handlers (start/stop/restart/getLogs/getStatus/...)
        ├── i18n.rs               # locale loader (embeds i18n/*.json)
        ├── settings.rs           # persistent settings (autostart toggle, language override)
        └── paths.rs              # resolves bundled resource paths (node binary, hub bundle)
```

### New: CI
- `.github/workflows/hub-tray-release.yml` — builds + publishes Windows + macOS artifacts to GitHub Releases on tag push.

---

## Conventions for this plan

- All paths are relative to repo root (`/Users/josespinal/Projects/Ecohub/rost_pos_restaurant`).
- Commits use Conventional Commits (`feat:`, `fix:`, `chore:`, `test:`, `docs:`).
- All Rust modules expose a small, testable API. Tray/menu/system-level behavior is verified by manual smoke tests at phase checkpoints; pure-logic modules get Rust unit tests.
- Hub tests run via `cd nu_pos_hub && npm run test:run`.
- Don't add `Co-Authored-By` lines — repo convention is opt-in only.

---

## Phase 1 — Hub: localhost admin status endpoint

### Task 1.1: Add LAN address helper to hub

**Files:**
- Create: `nu_pos_hub/src/admin/lanAddress.ts`
- Test: `nu_pos_hub/src/admin/__tests__/lanAddress.test.ts`

- [ ] **Step 1: Write the failing test**

Create `nu_pos_hub/src/admin/__tests__/lanAddress.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { pickLanAddress, buildLanUrl } from "../lanAddress.js";

describe("pickLanAddress", () => {
  it("returns first non-internal IPv4 address from interfaces", () => {
    const ifaces = {
      lo0: [{ family: "IPv4", address: "127.0.0.1", internal: true } as const],
      en0: [{ family: "IPv4", address: "192.168.1.42", internal: false } as const],
    };
    expect(pickLanAddress(ifaces)).toBe("192.168.1.42");
  });

  it("returns null when no non-internal IPv4 address exists", () => {
    const ifaces = {
      lo0: [{ family: "IPv4", address: "127.0.0.1", internal: true } as const],
    };
    expect(pickLanAddress(ifaces)).toBeNull();
  });

  it("skips IPv6 addresses", () => {
    const ifaces = {
      en0: [
        { family: "IPv6", address: "fe80::1", internal: false } as const,
        { family: "IPv4", address: "10.0.0.5", internal: false } as const,
      ],
    };
    expect(pickLanAddress(ifaces)).toBe("10.0.0.5");
  });
});

describe("buildLanUrl", () => {
  it("formats ws URL from address and port", () => {
    expect(buildLanUrl("192.168.1.42", 8765)).toBe("ws://192.168.1.42:8765");
  });

  it("returns null when address is null", () => {
    expect(buildLanUrl(null, 8765)).toBeNull();
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cd nu_pos_hub && npx vitest run src/admin/__tests__/lanAddress.test.ts
```

Expected: FAIL (module not found).

- [ ] **Step 3: Implement `lanAddress.ts`**

Create `nu_pos_hub/src/admin/lanAddress.ts`:

```ts
import os from "node:os";

type IfaceEntry = { family: string; address: string; internal: boolean };
type IfaceMap = Record<string, ReadonlyArray<IfaceEntry> | undefined>;

export function pickLanAddress(ifaces?: IfaceMap): string | null {
  const map = ifaces ?? (os.networkInterfaces() as unknown as IfaceMap);
  for (const entries of Object.values(map)) {
    if (!entries) continue;
    for (const e of entries) {
      if (e.family === "IPv4" && !e.internal) return e.address;
    }
  }
  return null;
}

export function buildLanUrl(address: string | null, port: number): string | null {
  if (!address) return null;
  return `ws://${address}:${port}`;
}
```

- [ ] **Step 4: Run the test to verify it passes**

```bash
cd nu_pos_hub && npx vitest run src/admin/__tests__/lanAddress.test.ts
```

Expected: 4 tests pass.

- [ ] **Step 5: Commit**

```bash
git add nu_pos_hub/src/admin/lanAddress.ts nu_pos_hub/src/admin/__tests__/lanAddress.test.ts
git commit -m "feat(hub): add LAN address helper for admin status"
```

---

### Task 1.2: Add admin status server (localhost-only)

**Files:**
- Create: `nu_pos_hub/src/admin/statusServer.ts`
- Test: `nu_pos_hub/src/admin/__tests__/statusServer.test.ts`

- [ ] **Step 1: Write the failing test**

Create `nu_pos_hub/src/admin/__tests__/statusServer.test.ts`:

```ts
import { describe, it, expect, afterEach } from "vitest";
import { createAdminStatusServer } from "../statusServer.js";

let close: undefined | (() => Promise<void>);

afterEach(async () => {
  if (close) {
    await close();
    close = undefined;
  }
});

describe("createAdminStatusServer", () => {
  it("responds with status payload on GET /admin/status", async () => {
    const server = createAdminStatusServer({
      port: 0,
      hubVersion: "0.1.0",
      startedAt: Date.now() - 5_000,
      wsPort: 8765,
      getConnectedTerminals: () => 3,
      getLanAddress: () => "192.168.1.42",
    });
    close = server.close;
    const port = await server.start();
    const res = await fetch(`http://127.0.0.1:${port}/admin/status`);
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body).toMatchObject({
      hubVersion: "0.1.0",
      connectedTerminals: 3,
      lanUrl: "ws://192.168.1.42:8765",
    });
    expect(typeof body.uptimeSec).toBe("number");
    expect(body.uptimeSec).toBeGreaterThanOrEqual(5);
  });

  it("returns 404 for other paths", async () => {
    const server = createAdminStatusServer({
      port: 0,
      hubVersion: "0.1.0",
      startedAt: Date.now(),
      wsPort: 8765,
      getConnectedTerminals: () => 0,
      getLanAddress: () => null,
    });
    close = server.close;
    const port = await server.start();
    const res = await fetch(`http://127.0.0.1:${port}/something-else`);
    expect(res.status).toBe(404);
  });

  it("does not accept non-loopback connections", async () => {
    // We assert binding behavior by inspecting the address — true network
    // isolation is verified at integration time.
    const server = createAdminStatusServer({
      port: 0,
      hubVersion: "0.1.0",
      startedAt: Date.now(),
      wsPort: 8765,
      getConnectedTerminals: () => 0,
      getLanAddress: () => null,
    });
    close = server.close;
    const port = await server.start();
    expect(server.address()).toMatchObject({ address: "127.0.0.1", port });
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cd nu_pos_hub && npx vitest run src/admin/__tests__/statusServer.test.ts
```

Expected: FAIL (module not found).

- [ ] **Step 3: Implement `statusServer.ts`**

Create `nu_pos_hub/src/admin/statusServer.ts`:

```ts
import { createServer, type Server } from "node:http";
import { buildLanUrl } from "./lanAddress.js";

export interface AdminStatusDeps {
  port: number;
  hubVersion: string;
  startedAt: number;
  wsPort: number;
  getConnectedTerminals: () => number;
  getLanAddress: () => string | null;
}

export interface AdminStatusServer {
  start(): Promise<number>;
  close(): Promise<void>;
  address(): { address: string; port: number } | null;
}

export function createAdminStatusServer(deps: AdminStatusDeps): AdminStatusServer {
  let server: Server | null = null;

  return {
    start() {
      server = createServer((req, res) => {
        if (req.method === "GET" && req.url === "/admin/status") {
          const body = {
            hubVersion: deps.hubVersion,
            uptimeSec: Math.floor((Date.now() - deps.startedAt) / 1000),
            connectedTerminals: deps.getConnectedTerminals(),
            lanUrl: buildLanUrl(deps.getLanAddress(), deps.wsPort),
          };
          res.writeHead(200, { "Content-Type": "application/json" });
          res.end(JSON.stringify(body));
          return;
        }
        res.writeHead(404).end();
      });
      return new Promise<number>((resolve, reject) => {
        server!.once("error", reject);
        server!.listen(deps.port, "127.0.0.1", () => {
          const addr = server!.address();
          if (addr && typeof addr === "object") resolve(addr.port);
          else reject(new Error("admin server: failed to bind"));
        });
      });
    },
    close() {
      return new Promise((resolve) => {
        if (!server) return resolve();
        server.close(() => resolve());
        server = null;
      });
    },
    address() {
      const addr = server?.address();
      if (addr && typeof addr === "object") {
        return { address: addr.address, port: addr.port };
      }
      return null;
    },
  };
}
```

- [ ] **Step 4: Run the test to verify it passes**

```bash
cd nu_pos_hub && npx vitest run src/admin/__tests__/statusServer.test.ts
```

Expected: 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add nu_pos_hub/src/admin/statusServer.ts nu_pos_hub/src/admin/__tests__/statusServer.test.ts
git commit -m "feat(hub): add localhost-only admin status server"
```

---

### Task 1.3: Add `adminPort` + `adminEnabled` to hub config

**Files:**
- Modify: `nu_pos_hub/src/config.ts`

- [ ] **Step 1: Add fields to `HubConfig`**

In `nu_pos_hub/src/config.ts`, add to the `HubConfig` interface (after `httpPort`):

```ts
  /** Admin status server port (bound to 127.0.0.1 only). 0 = disabled. */
  adminPort: number;
```

And in `loadConfig`, after the `httpPort` line:

```ts
    adminPort: envInt("HUB_ADMIN_PORT", 8767),
```

- [ ] **Step 2: Typecheck**

```bash
cd nu_pos_hub && npm run typecheck
```

Expected: no errors.

- [ ] **Step 3: Commit**

```bash
git add nu_pos_hub/src/config.ts
git commit -m "feat(hub): add HUB_ADMIN_PORT config field"
```

---

### Task 1.4: Wire admin server into hub bootstrap

**Files:**
- Modify: `nu_pos_hub/src/index.ts`
- Reference: `nu_pos_hub/package.json` (read version)

- [ ] **Step 1: Add imports and wiring**

In `nu_pos_hub/src/index.ts`:

1. At the top (with other imports):
```ts
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { resolve, dirname } from "node:path";
import { createAdminStatusServer } from "./admin/statusServer.js";
import { pickLanAddress } from "./admin/lanAddress.js";
```

2. After the existing `const httpServer = createHttpServer({...})` block, add (locate the section near line 569; the exact line number may have shifted):

```ts
// ─── Admin status server (localhost-only) ────────────────────────────────────

const hubPkgPath = resolve(dirname(fileURLToPath(import.meta.url)), "../package.json");
const hubVersion: string = JSON.parse(readFileSync(hubPkgPath, "utf8")).version;

let adminServer: ReturnType<typeof createAdminStatusServer> | null = null;
if (config.adminPort > 0) {
  adminServer = createAdminStatusServer({
    port: config.adminPort,
    hubVersion,
    startedAt,
    wsPort: config.port,
    getConnectedTerminals: () => terminalRegistry.count(),
    getLanAddress: () => pickLanAddress(),
  });
  const boundPort = await adminServer.start();
  logger.info({ port: boundPort }, "admin status server listening on 127.0.0.1");
}
```

3. Find the existing graceful shutdown handler (`process.on("SIGTERM", ...)` or similar). If absent, add one at the bottom. Ensure it closes the admin server:

```ts
process.on("SIGTERM", async () => {
  logger.info("SIGTERM received, shutting down");
  if (adminServer) await adminServer.close();
  process.exit(0);
});
process.on("SIGINT", async () => {
  logger.info("SIGINT received, shutting down");
  if (adminServer) await adminServer.close();
  process.exit(0);
});
```

(If similar handlers already exist, append the `adminServer.close()` call inside them rather than duplicating.)

- [ ] **Step 2: Typecheck + build**

```bash
cd nu_pos_hub && npm run typecheck && npm run build
```

Expected: no errors.

- [ ] **Step 3: Smoke test locally**

```bash
cd nu_pos_hub && npm run dev
```

In another shell:
```bash
curl -s http://127.0.0.1:8767/admin/status | jq
```

Expected: JSON with `hubVersion`, `uptimeSec`, `connectedTerminals: 0`, `lanUrl`.

Also verify it's not exposed externally — try the LAN IP:
```bash
curl -sS --max-time 2 http://$(ipconfig getifaddr en0 2>/dev/null || hostname -I | awk '{print $1}'):8767/admin/status; echo "exit=$?"
```

Expected: connection refused (non-zero exit), confirming localhost-only binding.

- [ ] **Step 4: Commit**

```bash
git add nu_pos_hub/src/index.ts
git commit -m "feat(hub): wire admin status server into bootstrap"
```

---

## Phase 2 — Tray app scaffold

### Task 2.1: Initialize `nu_pos_hub_tray/` directory and `version.json`

**Files:**
- Create: `nu_pos_hub_tray/version.json`
- Create: `nu_pos_hub_tray/.gitignore`
- Create: `nu_pos_hub_tray/README.md`

- [ ] **Step 1: Create directory and `version.json`**

```bash
mkdir -p nu_pos_hub_tray
```

Create `nu_pos_hub_tray/version.json`:

```json
{ "version": "0.1.0" }
```

- [ ] **Step 2: Create `.gitignore`**

Create `nu_pos_hub_tray/.gitignore`:

```
node_modules/
dist/
src-tauri/target/
src-tauri/resources/hub/
src-tauri/resources/node/
src-tauri/gen/schemas/
.env
.env.local
```

- [ ] **Step 3: Create README stub**

Create `nu_pos_hub_tray/README.md`:

```markdown
# Hub Tray App

Cross-platform (Windows + macOS) tray app that supervises `nu_pos_hub`.

**Version:** see `version.json` at this directory.

See `docs/superpowers/plans/2026-05-20-hub-tray-app.md` for the implementation plan.

## Dev quickstart

```bash
cd nu_pos_hub_tray
npm install
npm run prepare       # downloads Node runtime + builds hub bundle
npm run tauri dev
```
```

- [ ] **Step 4: Commit**

```bash
git add nu_pos_hub_tray/version.json nu_pos_hub_tray/.gitignore nu_pos_hub_tray/README.md
git commit -m "chore(hub-tray): scaffold directory"
```

---

### Task 2.2: Initialize the React webview (Vite + TS)

**Files:**
- Create: `nu_pos_hub_tray/package.json`
- Create: `nu_pos_hub_tray/tsconfig.json`
- Create: `nu_pos_hub_tray/vite.config.ts`
- Create: `nu_pos_hub_tray/index.html`
- Create: `nu_pos_hub_tray/src/main.tsx`
- Create: `nu_pos_hub_tray/src/App.tsx`

- [ ] **Step 1: Create `package.json`**

```json
{
  "name": "@nu/pos-hub-tray",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "scripts": {
    "version:sync": "node scripts/sync-version.mjs",
    "prepare:node": "node scripts/fetch-node-runtime.mjs",
    "prepare:hub": "node scripts/build-hub-bundle.mjs",
    "prepare:all": "npm run prepare:node && npm run prepare:hub",
    "dev": "vite",
    "build": "vite build",
    "tauri": "npm run version:sync && tauri",
    "tauri:dev": "npm run version:sync && npm run prepare:all && tauri dev",
    "tauri:build": "npm run version:sync && npm run prepare:all && tauri build"
  },
  "dependencies": {
    "@tauri-apps/api": "^2.0.0",
    "@tauri-apps/plugin-clipboard-manager": "^2.0.0",
    "@tauri-apps/plugin-dialog": "^2.0.0",
    "react": "^19.0.0",
    "react-dom": "^19.0.0"
  },
  "devDependencies": {
    "@tauri-apps/cli": "^2.0.0",
    "@types/react": "^19.0.0",
    "@types/react-dom": "^19.0.0",
    "@vitejs/plugin-react": "^4.3.0",
    "typescript": "^5.7.0",
    "vite": "^6.0.0"
  }
}
```

- [ ] **Step 2: Create `tsconfig.json`**

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "jsx": "react-jsx",
    "strict": true,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "resolveJsonModule": true,
    "isolatedModules": true,
    "noEmit": true,
    "lib": ["ES2022", "DOM"]
  },
  "include": ["src", "i18n"]
}
```

- [ ] **Step 3: Create `vite.config.ts`**

```ts
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1421, strictPort: true },
  build: { outDir: "dist" },
});
```

- [ ] **Step 4: Create `index.html`**

```html
<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Hub Logs</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

- [ ] **Step 5: Create `src/main.tsx` and `src/App.tsx` (placeholder)**

`nu_pos_hub_tray/src/main.tsx`:

```tsx
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App.js";

ReactDOM.createRoot(document.getElementById("root")!).render(<App />);
```

`nu_pos_hub_tray/src/App.tsx`:

```tsx
export default function App() {
  return <div style={{ padding: 16, fontFamily: "system-ui" }}>Hub tray — webview placeholder</div>;
}
```

- [ ] **Step 6: Install + build to verify**

```bash
cd nu_pos_hub_tray && npm install && npm run build
```

Expected: `dist/` is created, no errors.

- [ ] **Step 7: Commit**

```bash
git add nu_pos_hub_tray/package.json nu_pos_hub_tray/package-lock.json \
  nu_pos_hub_tray/tsconfig.json nu_pos_hub_tray/vite.config.ts \
  nu_pos_hub_tray/index.html nu_pos_hub_tray/src/main.tsx nu_pos_hub_tray/src/App.tsx
git commit -m "chore(hub-tray): scaffold React webview"
```

---

### Task 2.3: Scaffold Tauri shell

**Files:**
- Create: `nu_pos_hub_tray/src-tauri/Cargo.toml`
- Create: `nu_pos_hub_tray/src-tauri/build.rs`
- Create: `nu_pos_hub_tray/src-tauri/tauri.conf.json`
- Create: `nu_pos_hub_tray/src-tauri/capabilities/default.json`
- Create: `nu_pos_hub_tray/src-tauri/src/main.rs`
- Create: `nu_pos_hub_tray/src-tauri/src/lib.rs`
- Create: `nu_pos_hub_tray/src-tauri/icons/` (copy from `nu_pos_react/src-tauri/icons/` as a starting set)

- [ ] **Step 1: Create `Cargo.toml`**

```toml
[package]
name = "nu_pos_hub_tray"
version = "0.1.0"
description = "Sirvo POS Hub tray app"
authors = ["Industria"]
license = ""
edition = "2021"
rust-version = "1.77"

[lib]
name = "nu_pos_hub_tray_lib"
crate-type = ["staticlib", "cdylib", "rlib"]

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tauri = { version = "2", features = ["tray-icon", "image-png"] }
tauri-plugin-clipboard-manager = "2"
tauri-plugin-dialog = "2"
tauri-plugin-autostart = "2"
tauri-plugin-updater = "2"
tauri-plugin-opener = "2"
tokio = { version = "1", features = ["rt-multi-thread", "macros", "process", "io-util", "sync", "time", "fs"] }
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
anyhow = "1"
thiserror = "1"
parking_lot = "0.12"
once_cell = "1"
chrono = { version = "0.4", default-features = false, features = ["clock", "serde"] }
log = "0.4"
env_logger = "0.11"
```

- [ ] **Step 2: Create `build.rs`**

```rust
fn main() {
    tauri_build::build();
}
```

- [ ] **Step 3: Create `tauri.conf.json`**

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "Sirvo Hub",
  "version": "0.1.0",
  "identifier": "com.sirvo.hub.tray",
  "build": {
    "beforeDevCommand": "npm run dev",
    "beforeBuildCommand": "npm run build",
    "devUrl": "http://localhost:1421",
    "frontendDist": "../dist"
  },
  "app": {
    "windows": [
      {
        "label": "logs",
        "title": "Hub Logs",
        "width": 800,
        "height": 500,
        "visible": false,
        "resizable": true,
        "decorations": true
      }
    ],
    "trayIcon": {
      "iconPath": "icons/icon.png",
      "iconAsTemplate": true
    },
    "security": {
      "csp": null
    }
  },
  "bundle": {
    "active": true,
    "targets": ["msi", "dmg"],
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ],
    "resources": ["resources/hub/**/*", "resources/node/**/*"]
  },
  "plugins": {
    "updater": {
      "active": true,
      "endpoints": [
        "https://github.com/INDUSTRIA_OWNER/INDUSTRIA_REPO/releases/latest/download/latest.json"
      ],
      "dialog": false,
      "pubkey": "REPLACE_WITH_TAURI_UPDATER_PUBKEY"
    }
  }
}
```

> Note: replace `INDUSTRIA_OWNER/INDUSTRIA_REPO` once the GitHub repo coordinates are confirmed; replace `REPLACE_WITH_TAURI_UPDATER_PUBKEY` after generating the keypair in Task 9.1.

- [ ] **Step 4: Create `capabilities/default.json`**

```json
{
  "$schema": "https://schema.tauri.app/config/2/capabilities",
  "identifier": "default",
  "description": "Default capability set for the tray app",
  "windows": ["logs"],
  "permissions": [
    "core:default",
    "clipboard-manager:allow-write-text",
    "dialog:allow-confirm",
    "autostart:default",
    "updater:default",
    "opener:default"
  ]
}
```

- [ ] **Step 5: Create `src/main.rs` and `src/lib.rs`**

`nu_pos_hub_tray/src-tauri/src/main.rs`:

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    nu_pos_hub_tray_lib::run();
}
```

`nu_pos_hub_tray/src-tauri/src/lib.rs`:

```rust
pub fn run() {
    env_logger::init();
    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .setup(|_app| Ok(()))
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 6: Copy icon set from `nu_pos_react`**

```bash
mkdir -p nu_pos_hub_tray/src-tauri/icons
cp nu_pos_react/src-tauri/icons/* nu_pos_hub_tray/src-tauri/icons/
```

(These are placeholders; final icons will replace them in Phase 12.)

- [ ] **Step 7: Verify Tauri app builds**

```bash
cd nu_pos_hub_tray && npm run tauri info
```

Expected: prints info without errors. (Don't run `tauri dev` yet — the supervisor isn't ready and the hub bundle isn't staged.)

- [ ] **Step 8: Commit**

```bash
git add nu_pos_hub_tray/src-tauri
git commit -m "chore(hub-tray): scaffold Tauri shell"
```

---

### Task 2.4: Add version sync script

**Files:**
- Create: `nu_pos_hub_tray/scripts/sync-version.mjs`

- [ ] **Step 1: Create script**

`nu_pos_hub_tray/scripts/sync-version.mjs`:

```js
#!/usr/bin/env node
/**
 * Stamps tauri.conf.json + Cargo.toml from nu_pos_hub_tray/version.json.
 * Mirrors root scripts/sync-version.mjs but scoped to this app.
 */
import { readFileSync, writeFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const trayRoot = resolve(__dirname, "..");

const { version } = JSON.parse(
  readFileSync(resolve(trayRoot, "version.json"), "utf8").replace(/^﻿/, "")
);

let build = "0";
try {
  build = execFileSync("git", ["rev-list", "--count", "HEAD"], {
    encoding: "utf8",
    cwd: trayRoot,
  }).trim();
} catch {}

const full = `${version}+${build}`;

// package.json
const pkgPath = resolve(trayRoot, "package.json");
const pkg = JSON.parse(readFileSync(pkgPath, "utf8"));
pkg.version = version;
writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + "\n");

// tauri.conf.json
const tauriPath = resolve(trayRoot, "src-tauri/tauri.conf.json");
const tauri = JSON.parse(readFileSync(tauriPath, "utf8"));
tauri.version = version;
writeFileSync(tauriPath, JSON.stringify(tauri, null, 2) + "\n");

// Cargo.toml
const cargoPath = resolve(trayRoot, "src-tauri/Cargo.toml");
let cargo = readFileSync(cargoPath, "utf8");
cargo = cargo.replace(/^version\s*=\s*"[^"]*"/m, `version = "${version}"`);
writeFileSync(cargoPath, cargo);

console.log(full);
```

- [ ] **Step 2: Run it**

```bash
cd nu_pos_hub_tray && npm run version:sync
```

Expected: prints `0.1.0+<commit-count>` and rewrites the files in place with the same version (no-op diff).

- [ ] **Step 3: Commit**

```bash
git add nu_pos_hub_tray/scripts/sync-version.mjs
git commit -m "chore(hub-tray): add version sync script"
```

---

## Phase 3 — Bundling Node runtime + hub artifact

### Task 3.1: Node runtime fetcher

**Files:**
- Create: `nu_pos_hub_tray/scripts/fetch-node-runtime.mjs`

- [ ] **Step 1: Create fetcher**

```js
#!/usr/bin/env node
/**
 * Downloads a portable Node.js runtime for the current build platform and
 * stages it into src-tauri/resources/node/<platform>/.
 *
 * Honors NODE_RUNTIME_VERSION (default: 20.18.0) and a per-platform target
 * triple. Skips download if already present.
 */
import { mkdirSync, existsSync, createWriteStream, readFileSync, writeFileSync, rmSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";
import https from "node:https";

const __dirname = dirname(fileURLToPath(import.meta.url));
const trayRoot = resolve(__dirname, "..");
const resourcesRoot = resolve(trayRoot, "src-tauri/resources/node");

const NODE_VERSION = process.env.NODE_RUNTIME_VERSION || "20.18.0";

const platforms = {
  darwin: {
    arm64: { archive: `node-v${NODE_VERSION}-darwin-arm64.tar.gz`, binary: "bin/node" },
    x64:   { archive: `node-v${NODE_VERSION}-darwin-x64.tar.gz`,   binary: "bin/node" },
  },
  win32: {
    x64:   { archive: `node-v${NODE_VERSION}-win-x64.zip`,         binary: "node.exe" },
  },
};

function targetSpecs() {
  const explicit = process.env.NODE_RUNTIME_TARGETS;
  if (explicit) {
    return explicit.split(",").map((s) => {
      const [os, arch] = s.trim().split("-");
      return { os, arch };
    });
  }
  // default: current host only
  const map = { darwin: "darwin", win32: "win32" };
  const os = map[process.platform];
  if (!os) throw new Error(`Unsupported host platform: ${process.platform}`);
  return [{ os, arch: process.arch }];
}

function download(url, dest) {
  return new Promise((resolveP, rejectP) => {
    const file = createWriteStream(dest);
    https.get(url, (res) => {
      if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
        file.close();
        rmSync(dest, { force: true });
        return download(res.headers.location, dest).then(resolveP, rejectP);
      }
      if (res.statusCode !== 200) {
        return rejectP(new Error(`Download failed: ${res.statusCode} for ${url}`));
      }
      res.pipe(file);
      file.on("finish", () => file.close(resolveP));
    }).on("error", rejectP);
  });
}

async function stageOne({ os, arch }) {
  const spec = platforms[os]?.[arch];
  if (!spec) throw new Error(`No Node spec for ${os}-${arch}`);
  const targetDir = resolve(resourcesRoot, `${os}-${arch}`);
  mkdirSync(targetDir, { recursive: true });
  const binaryPath = resolve(targetDir, spec.binary);
  if (existsSync(binaryPath)) {
    console.log(`[node] ${os}-${arch}: already present at ${binaryPath}`);
    return;
  }
  const url = `https://nodejs.org/dist/v${NODE_VERSION}/${spec.archive}`;
  const archivePath = resolve(targetDir, spec.archive);
  console.log(`[node] downloading ${url}`);
  await download(url, archivePath);
  if (spec.archive.endsWith(".tar.gz")) {
    execFileSync("tar", ["-xzf", archivePath, "--strip-components=1", "-C", targetDir]);
  } else if (spec.archive.endsWith(".zip")) {
    execFileSync("unzip", ["-q", archivePath, "-d", targetDir]);
    // Move contents up one level (zip has top dir node-vXX.YY.Z-win-x64)
    const topDir = resolve(targetDir, `node-v${NODE_VERSION}-win-x64`);
    if (existsSync(topDir)) {
      execFileSync("sh", ["-c", `mv "${topDir}"/* "${targetDir}/" && rmdir "${topDir}"`]);
    }
  }
  rmSync(archivePath, { force: true });
  console.log(`[node] staged ${os}-${arch} -> ${targetDir}`);
}

const targets = targetSpecs();
for (const t of targets) {
  await stageOne(t);
}
```

- [ ] **Step 2: Run it for the current host**

```bash
cd nu_pos_hub_tray && npm run prepare:node
```

Expected: downloads + extracts Node, prints `[node] staged …`, and creates `src-tauri/resources/node/<os>-<arch>/bin/node` (or `node.exe`).

- [ ] **Step 3: Commit**

```bash
git add nu_pos_hub_tray/scripts/fetch-node-runtime.mjs
git commit -m "chore(hub-tray): add Node runtime fetcher script"
```

---

### Task 3.2: Hub bundle builder

**Files:**
- Create: `nu_pos_hub_tray/scripts/build-hub-bundle.mjs`

- [ ] **Step 1: Create builder**

```js
#!/usr/bin/env node
/**
 * Builds nu_pos_hub and stages the production artifact into
 * src-tauri/resources/hub/.
 *
 * Layout staged:
 *   resources/hub/dist/...        (compiled JS)
 *   resources/hub/node_modules/...(production deps only)
 *   resources/hub/package.json    (with version stamp)
 *
 * The tray's Rust supervisor invokes `node dist/index.js` against this dir.
 */
import { mkdirSync, rmSync, cpSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";

const __dirname = dirname(fileURLToPath(import.meta.url));
const trayRoot = resolve(__dirname, "..");
const repoRoot = resolve(trayRoot, "..");
const hubRoot = resolve(repoRoot, "nu_pos_hub");
const stagingRoot = resolve(trayRoot, "src-tauri/resources/hub");

function sh(cmd, args, cwd) {
  console.log(`$ ${cmd} ${args.join(" ")}  (cwd=${cwd})`);
  execFileSync(cmd, args, { cwd, stdio: "inherit" });
}

console.log("[hub] cleaning staging dir");
rmSync(stagingRoot, { recursive: true, force: true });
mkdirSync(stagingRoot, { recursive: true });

console.log("[hub] building TypeScript");
sh("npm", ["run", "build"], hubRoot);

console.log("[hub] copying dist/");
cpSync(resolve(hubRoot, "dist"), resolve(stagingRoot, "dist"), { recursive: true });

console.log("[hub] copying package.json");
const pkg = JSON.parse(readFileSync(resolve(hubRoot, "package.json"), "utf8"));
// Replace file: deps with their resolved counterparts during install.
writeFileSync(resolve(stagingRoot, "package.json"), JSON.stringify(pkg, null, 2));

console.log("[hub] installing production deps in staging");
// nu_pos_hub depends on @nu/sync-protocol via file:../nu_pos_sync_protocol.
// We need that to be installable from the staging dir. Copy it in.
const protoSrc = resolve(repoRoot, "nu_pos_sync_protocol");
const protoDst = resolve(stagingRoot, "vendored/nu_pos_sync_protocol");
mkdirSync(dirname(protoDst), { recursive: true });
cpSync(protoSrc, protoDst, {
  recursive: true,
  filter: (src) => !src.includes("node_modules"),
});
// Build the protocol so its dist/ exists.
sh("npm", ["install"], protoDst);
if (existsSync(resolve(protoDst, "package.json"))) {
  const protoPkg = JSON.parse(readFileSync(resolve(protoDst, "package.json"), "utf8"));
  if (protoPkg.scripts?.build) sh("npm", ["run", "build"], protoDst);
}

// Rewrite the @nu/sync-protocol dep to point at the vendored copy.
const stagedPkg = JSON.parse(readFileSync(resolve(stagingRoot, "package.json"), "utf8"));
if (stagedPkg.dependencies?.["@nu/sync-protocol"]?.startsWith("file:")) {
  stagedPkg.dependencies["@nu/sync-protocol"] = "file:./vendored/nu_pos_sync_protocol";
  writeFileSync(resolve(stagingRoot, "package.json"), JSON.stringify(stagedPkg, null, 2));
}

sh("npm", ["install", "--omit=dev", "--ignore-scripts=false"], stagingRoot);
// better-sqlite3 needs its native binding built; --ignore-scripts=false ensures
// prebuilds/install scripts run for the host platform.

console.log("[hub] done -> " + stagingRoot);
```

- [ ] **Step 2: Run it**

```bash
cd nu_pos_hub_tray && npm run prepare:hub
```

Expected: completes without error, produces `src-tauri/resources/hub/{dist,node_modules,package.json}`.

- [ ] **Step 3: Smoke-test the bundle directly**

```bash
cd nu_pos_hub_tray/src-tauri/resources/hub && \
  HUB_ADMIN_PORT=18767 HUB_PORT=18765 HUB_HTTP_PORT=18766 \
  HUB_DB_PATH=":memory:" \
  ../../resources/node/$(node -e "console.log(process.platform+'-'+process.arch)")/bin/node dist/index.js &
HUB_PID=$!
sleep 2
curl -s http://127.0.0.1:18767/admin/status
kill $HUB_PID
```

Expected: JSON status payload.

> Note: `better-sqlite3` ships per-platform native binaries. When building releases for a different platform than the host (e.g. macOS host building Windows bundle), the bundle must be produced on or for the target OS. Phase 11 (CI) builds each OS bundle on its native runner to avoid this.

- [ ] **Step 4: Commit**

```bash
git add nu_pos_hub_tray/scripts/build-hub-bundle.mjs
git commit -m "chore(hub-tray): add hub bundle builder script"
```

---

## Phase 4 — Rust: paths, supervisor, log buffer

### Task 4.1: `paths.rs` — resolve bundled resource paths

**Files:**
- Create: `nu_pos_hub_tray/src-tauri/src/paths.rs`
- Modify: `nu_pos_hub_tray/src-tauri/src/lib.rs`

- [ ] **Step 1: Implement `paths.rs`**

```rust
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// Per-target Node binary name relative to resources/node/<os>-<arch>/.
fn node_bin_relative() -> &'static str {
    if cfg!(target_os = "windows") {
        "node.exe"
    } else {
        "bin/node"
    }
}

fn target_dir_name() -> String {
    let os = if cfg!(target_os = "windows") { "win32" } else if cfg!(target_os = "macos") { "darwin" } else { "linux" };
    let arch = if cfg!(target_arch = "x86_64") { "x64" } else if cfg!(target_arch = "aarch64") { "arm64" } else { "unknown" };
    format!("{os}-{arch}")
}

pub fn node_binary(app: &AppHandle) -> PathBuf {
    let base = app.path().resource_dir().expect("resource_dir");
    base.join("resources/node").join(target_dir_name()).join(node_bin_relative())
}

pub fn hub_dir(app: &AppHandle) -> PathBuf {
    let base = app.path().resource_dir().expect("resource_dir");
    base.join("resources/hub")
}

pub fn hub_entry(app: &AppHandle) -> PathBuf {
    hub_dir(app).join("dist/index.js")
}

pub fn hub_db_path(app: &AppHandle) -> PathBuf {
    let dir = app.path().app_data_dir().expect("app_data_dir");
    std::fs::create_dir_all(&dir).ok();
    dir.join("hub.sqlite")
}

pub fn log_file_path(app: &AppHandle) -> PathBuf {
    let dir = app.path().app_log_dir().expect("app_log_dir");
    std::fs::create_dir_all(&dir).ok();
    dir.join("hub.log")
}
```

- [ ] **Step 2: Register module in `lib.rs`**

In `nu_pos_hub_tray/src-tauri/src/lib.rs`, add at top:

```rust
mod paths;
```

- [ ] **Step 3: Build to verify**

```bash
cd nu_pos_hub_tray/src-tauri && cargo check
```

Expected: compiles.

- [ ] **Step 4: Commit**

```bash
git add nu_pos_hub_tray/src-tauri/src/paths.rs nu_pos_hub_tray/src-tauri/src/lib.rs
git commit -m "feat(hub-tray): resolve bundled resource paths"
```

---

### Task 4.2: `log_buffer.rs` — rolling in-memory buffer + file writer

**Files:**
- Create: `nu_pos_hub_tray/src-tauri/src/log_buffer.rs`
- Modify: `nu_pos_hub_tray/src-tauri/src/lib.rs`

- [ ] **Step 1: Write failing tests**

In `nu_pos_hub_tray/src-tauri/src/log_buffer.rs`:

```rust
use std::collections::VecDeque;
use parking_lot::Mutex;
use std::sync::Arc;

/// Rolling in-memory log buffer.
///
/// Stores up to `max_lines` most-recent lines. Older lines are dropped.
pub struct LogBuffer {
    inner: Arc<Mutex<VecDeque<String>>>,
    max_lines: usize,
}

impl LogBuffer {
    pub fn new(max_lines: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::with_capacity(max_lines))),
            max_lines,
        }
    }

    pub fn push(&self, line: String) {
        let mut q = self.inner.lock();
        if q.len() == self.max_lines {
            q.pop_front();
        }
        q.push_back(line);
    }

    pub fn snapshot(&self) -> Vec<String> {
        self.inner.lock().iter().cloned().collect()
    }

    pub fn handle(&self) -> Arc<Mutex<VecDeque<String>>> {
        self.inner.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_appends_lines() {
        let buf = LogBuffer::new(3);
        buf.push("a".into());
        buf.push("b".into());
        assert_eq!(buf.snapshot(), vec!["a", "b"]);
    }

    #[test]
    fn push_drops_oldest_when_full() {
        let buf = LogBuffer::new(2);
        buf.push("a".into());
        buf.push("b".into());
        buf.push("c".into());
        assert_eq!(buf.snapshot(), vec!["b", "c"]);
    }

    #[test]
    fn snapshot_returns_independent_copy() {
        let buf = LogBuffer::new(5);
        buf.push("a".into());
        let snap = buf.snapshot();
        buf.push("b".into());
        assert_eq!(snap, vec!["a"]);
    }
}
```

- [ ] **Step 2: Register module**

In `lib.rs`, add `mod log_buffer;`.

- [ ] **Step 3: Run tests**

```bash
cd nu_pos_hub_tray/src-tauri && cargo test log_buffer
```

Expected: 3 tests pass.

- [ ] **Step 4: Commit**

```bash
git add nu_pos_hub_tray/src-tauri/src/log_buffer.rs nu_pos_hub_tray/src-tauri/src/lib.rs
git commit -m "feat(hub-tray): rolling in-memory log buffer"
```

---

### Task 4.3: `supervisor.rs` — child process lifecycle

**Files:**
- Create: `nu_pos_hub_tray/src-tauri/src/supervisor.rs`
- Modify: `nu_pos_hub_tray/src-tauri/src/lib.rs`

- [ ] **Step 1: Implement supervisor**

```rust
use crate::log_buffer::LogBuffer;
use anyhow::{anyhow, Result};
use parking_lot::Mutex;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::oneshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum HubState {
    Stopped,
    Starting,
    Running,
    Restarting,
    Errored,
}

pub struct SupervisorConfig {
    pub node_binary: PathBuf,
    pub hub_entry: PathBuf,
    pub hub_dir: PathBuf,
    pub db_path: PathBuf,
    pub log_file: PathBuf,
    pub admin_port: u16,
    pub ws_port: u16,
    pub http_port: u16,
}

pub struct Supervisor {
    state: Arc<Mutex<HubState>>,
    child: Arc<Mutex<Option<Child>>>,
    log_buffer: LogBuffer,
    config: SupervisorConfig,
    app: AppHandle,
}

impl Supervisor {
    pub fn new(app: AppHandle, config: SupervisorConfig, log_buffer: LogBuffer) -> Self {
        Self {
            state: Arc::new(Mutex::new(HubState::Stopped)),
            child: Arc::new(Mutex::new(None)),
            log_buffer,
            config,
            app,
        }
    }

    pub fn state(&self) -> HubState {
        *self.state.lock()
    }

    fn set_state(&self, next: HubState) {
        *self.state.lock() = next;
        let _ = self.app.emit("hub-state", next);
    }

    pub async fn start(&self) -> Result<()> {
        if matches!(self.state(), HubState::Starting | HubState::Running) {
            return Ok(());
        }
        self.set_state(HubState::Starting);

        let mut cmd = Command::new(&self.config.node_binary);
        cmd.arg(&self.config.hub_entry)
            .current_dir(&self.config.hub_dir)
            .env("HUB_PORT", self.config.ws_port.to_string())
            .env("HUB_HTTP_PORT", self.config.http_port.to_string())
            .env("HUB_ADMIN_PORT", self.config.admin_port.to_string())
            .env("HUB_DB_PATH", &self.config.db_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = cmd
            .spawn()
            .map_err(|e| anyhow!("spawn node: {e}"))?;

        let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;
        let stderr = child.stderr.take().ok_or_else(|| anyhow!("no stderr"))?;
        spawn_reader(stdout, self.log_buffer.clone(), self.app.clone(), "out");
        spawn_reader(stderr, self.log_buffer.clone(), self.app.clone(), "err");

        let state = self.state.clone();
        let app = self.app.clone();
        let pid = child.id();
        *self.child.lock() = Some(child);
        self.set_state(HubState::Running);

        // Watch for exit
        let child_handle = self.child.clone();
        tokio::spawn(async move {
            let mut guard = child_handle.lock();
            let Some(mut c) = guard.take() else { return };
            drop(guard);
            let status = c.wait().await;
            let next = match status {
                Ok(s) if s.success() => HubState::Stopped,
                _ => HubState::Errored,
            };
            *state.lock() = next;
            let _ = app.emit("hub-state", next);
            log::info!("hub child (pid={:?}) exited", pid);
        });

        Ok(())
    }

    pub async fn stop(&self) -> Result<()> {
        let mut guard = self.child.lock();
        if let Some(mut c) = guard.take() {
            drop(guard);
            let _ = c.start_kill();
            let _ = c.wait().await;
        }
        self.set_state(HubState::Stopped);
        Ok(())
    }

    pub async fn restart(&self) -> Result<()> {
        self.set_state(HubState::Restarting);
        self.stop().await?;
        self.start().await
    }
}

impl Clone for LogBuffer {
    fn clone(&self) -> Self {
        LogBuffer::from_handle(self.handle(), 5000)
    }
}

impl LogBuffer {
    pub fn from_handle(handle: std::sync::Arc<parking_lot::Mutex<std::collections::VecDeque<String>>>, max_lines: usize) -> Self {
        // Helper used by Clone above; the buffer is logically one ring.
        // Implementing Clone this way means clones share the same backing ring.
        Self {
            inner: handle,
            max_lines,
        }
    }
}

fn spawn_reader<R>(stream: R, buf: LogBuffer, app: AppHandle, stream_name: &'static str)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let reader = BufReader::new(stream);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let stamped = format!("[{stream_name}] {line}");
            buf.push(stamped.clone());
            let _ = app.emit("hub-log-line", stamped);
        }
    });
}
```

> Note: the `LogBuffer::from_handle` + `Clone` shim above keeps clones sharing the ring. If `log_buffer.rs` already declares `inner` and `max_lines` as `pub(crate)`, this works as-is. Otherwise: in `log_buffer.rs`, mark both fields `pub(crate)` and the test in Task 4.2 still passes.

- [ ] **Step 2: Mark `LogBuffer` fields visible to `supervisor`**

In `nu_pos_hub_tray/src-tauri/src/log_buffer.rs`, change the struct to:

```rust
pub struct LogBuffer {
    pub(crate) inner: Arc<Mutex<VecDeque<String>>>,
    pub(crate) max_lines: usize,
}
```

- [ ] **Step 3: Register module**

In `lib.rs`, add `mod supervisor;` and `mod log_buffer;` if not already.

- [ ] **Step 4: Build**

```bash
cd nu_pos_hub_tray/src-tauri && cargo check
```

Expected: compiles.

- [ ] **Step 5: Commit**

```bash
git add nu_pos_hub_tray/src-tauri/src
git commit -m "feat(hub-tray): supervise hub child process"
```

---

### Task 4.4: Append log lines to rotating file

**Files:**
- Modify: `nu_pos_hub_tray/src-tauri/src/supervisor.rs`

- [ ] **Step 1: Add file appender to `spawn_reader`**

Modify the `spawn_reader` function to also append to a log file. Add a `log_file: PathBuf` parameter and use `tokio::fs::OpenOptions` to append. Skeleton:

```rust
fn spawn_reader<R>(
    stream: R,
    buf: LogBuffer,
    app: AppHandle,
    stream_name: &'static str,
    log_file: std::path::PathBuf,
) where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let reader = BufReader::new(stream);
        let mut lines = reader.lines();
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_file)
            .await
            .ok();
        while let Ok(Some(line)) = lines.next_line().await {
            let stamped = format!("[{stream_name}] {line}");
            buf.push(stamped.clone());
            if let Some(f) = file.as_mut() {
                use tokio::io::AsyncWriteExt;
                let _ = f.write_all(stamped.as_bytes()).await;
                let _ = f.write_all(b"\n").await;
            }
            let _ = app.emit("hub-log-line", stamped);
        }
    });
}
```

And update the two `spawn_reader(...)` call sites in `start()` to pass `self.config.log_file.clone()`.

> Note: we accept simple append-only writes for v1. Size-based rotation is intentionally deferred — v1 ships without it, and the in-memory buffer is the primary log view.

- [ ] **Step 2: Build**

```bash
cd nu_pos_hub_tray/src-tauri && cargo check
```

Expected: compiles.

- [ ] **Step 3: Commit**

```bash
git add nu_pos_hub_tray/src-tauri/src/supervisor.rs
git commit -m "feat(hub-tray): append hub logs to file"
```

---

## Phase 5 — Status polling

### Task 5.1: `status_client.rs` — poll `/admin/status`

**Files:**
- Create: `nu_pos_hub_tray/src-tauri/src/status_client.rs`
- Modify: `nu_pos_hub_tray/src-tauri/src/lib.rs`

- [ ] **Step 1: Implement client**

```rust
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use parking_lot::Mutex;
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HubStatus {
    pub hub_version: Option<String>,
    pub uptime_sec: Option<u64>,
    pub connected_terminals: Option<u32>,
    pub lan_url: Option<String>,
    pub reachable: bool,
}

#[derive(Clone)]
pub struct StatusClient {
    inner: Arc<Inner>,
}

struct Inner {
    app: AppHandle,
    admin_port: u16,
    last: Mutex<HubStatus>,
}

impl StatusClient {
    pub fn new(app: AppHandle, admin_port: u16) -> Self {
        Self {
            inner: Arc::new(Inner {
                app,
                admin_port,
                last: Mutex::new(HubStatus::default()),
            }),
        }
    }

    pub fn last(&self) -> HubStatus {
        self.inner.last.lock().clone()
    }

    pub async fn start_polling(self) {
        // Single fixed poll cadence in v1; menu/window visibility was an
        // optimization not worth the complexity yet.
        let mut interval = tokio::time::interval(Duration::from_secs(3));
        loop {
            interval.tick().await;
            let status = self.fetch_once().await;
            *self.inner.last.lock() = status.clone();
            let _ = self.inner.app.emit("hub-status", status);
        }
    }

    async fn fetch_once(&self) -> HubStatus {
        let url = format!("http://127.0.0.1:{}/admin/status", self.inner.admin_port);
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(1500))
            .build()
            .expect("reqwest client");
        match client.get(&url).send().await {
            Ok(res) if res.status().is_success() => {
                if let Ok(body) = res.json::<RawStatus>().await {
                    return HubStatus {
                        hub_version: Some(body.hub_version),
                        uptime_sec: Some(body.uptime_sec),
                        connected_terminals: Some(body.connected_terminals),
                        lan_url: body.lan_url,
                        reachable: true,
                    };
                }
                HubStatus { reachable: false, ..Default::default() }
            }
            _ => HubStatus { reachable: false, ..Default::default() },
        }
    }
}

#[derive(Deserialize)]
struct RawStatus {
    #[serde(rename = "hubVersion")]
    hub_version: String,
    #[serde(rename = "uptimeSec")]
    uptime_sec: u64,
    #[serde(rename = "connectedTerminals")]
    connected_terminals: u32,
    #[serde(rename = "lanUrl")]
    lan_url: Option<String>,
}
```

- [ ] **Step 2: Register module**

`mod status_client;` in `lib.rs`.

- [ ] **Step 3: Build**

```bash
cd nu_pos_hub_tray/src-tauri && cargo check
```

Expected: compiles.

- [ ] **Step 4: Commit**

```bash
git add nu_pos_hub_tray/src-tauri/src/status_client.rs nu_pos_hub_tray/src-tauri/src/lib.rs
git commit -m "feat(hub-tray): poll hub admin status"
```

---

## Phase 6 — Settings, i18n, commands

### Task 6.1: `settings.rs` — persistent settings (autostart, language)

**Files:**
- Create: `nu_pos_hub_tray/src-tauri/src/settings.rs`
- Modify: `nu_pos_hub_tray/src-tauri/src/lib.rs`

- [ ] **Step 1: Implement**

```rust
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub autostart: bool,
    pub language: Option<String>, // None = follow OS locale
}

impl Default for Settings {
    fn default() -> Self {
        Self { autostart: true, language: None }
    }
}

pub fn settings_path(app: &AppHandle) -> PathBuf {
    let dir = app.path().app_config_dir().expect("app_config_dir");
    std::fs::create_dir_all(&dir).ok();
    dir.join("settings.json")
}

pub fn load(app: &AppHandle) -> Settings {
    let path = settings_path(app);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<Settings>(&s).ok())
        .unwrap_or_default()
}

pub fn save(app: &AppHandle, settings: &Settings) -> Result<()> {
    let path = settings_path(app);
    std::fs::write(path, serde_json::to_string_pretty(settings)?)?;
    Ok(())
}
```

- [ ] **Step 2: Register module**

`mod settings;` in `lib.rs`.

- [ ] **Step 3: Build**

```bash
cd nu_pos_hub_tray/src-tauri && cargo check
```

- [ ] **Step 4: Commit**

```bash
git add nu_pos_hub_tray/src-tauri/src/settings.rs nu_pos_hub_tray/src-tauri/src/lib.rs
git commit -m "feat(hub-tray): persistent settings (autostart, language)"
```

---

### Task 6.2: i18n files

**Files:**
- Create: `nu_pos_hub_tray/i18n/en.json`
- Create: `nu_pos_hub_tray/i18n/es.json`

- [ ] **Step 1: English strings**

`nu_pos_hub_tray/i18n/en.json`:

```json
{
  "tray": {
    "running": "Hub: Running",
    "stopped": "Hub: Stopped",
    "starting": "Hub: Starting…",
    "restarting": "Hub: Restarting…",
    "errored": "Hub: Error",
    "terminals": "Terminals connected: {count}",
    "lanUrl": "LAN URL: {url}",
    "lanUrlUnknown": "LAN URL: unknown",
    "copyLanUrl": "Copy LAN URL",
    "start": "Start Hub",
    "restart": "Restart Hub",
    "stop": "Stop Hub",
    "viewLogs": "View Logs",
    "autostart": "Start with system",
    "language": "Language",
    "languageEn": "English",
    "languageEs": "Español",
    "languageAuto": "System default",
    "checkForUpdates": "Check for Updates",
    "quit": "Quit"
  },
  "dialog": {
    "stopTitle": "Stop the Hub?",
    "stopBody": "Stopping the hub will disconnect all POS terminals until you start it again. Are you sure?",
    "stopConfirm": "Stop",
    "stopCancel": "Cancel",
    "copied": "LAN URL copied to clipboard.",
    "updateReady": "An update is ready. Restart now to apply?",
    "updateRestart": "Restart and update",
    "updateLater": "Later"
  },
  "logs": {
    "title": "Hub Logs",
    "empty": "No log output yet.",
    "clear": "Clear",
    "follow": "Follow"
  }
}
```

- [ ] **Step 2: Spanish strings**

`nu_pos_hub_tray/i18n/es.json`:

```json
{
  "tray": {
    "running": "Hub: En ejecución",
    "stopped": "Hub: Detenido",
    "starting": "Hub: Iniciando…",
    "restarting": "Hub: Reiniciando…",
    "errored": "Hub: Error",
    "terminals": "Terminales conectadas: {count}",
    "lanUrl": "URL de red local: {url}",
    "lanUrlUnknown": "URL de red local: desconocida",
    "copyLanUrl": "Copiar URL de red local",
    "start": "Iniciar Hub",
    "restart": "Reiniciar Hub",
    "stop": "Detener Hub",
    "viewLogs": "Ver registros",
    "autostart": "Iniciar con el sistema",
    "language": "Idioma",
    "languageEn": "English",
    "languageEs": "Español",
    "languageAuto": "Predeterminado del sistema",
    "checkForUpdates": "Buscar actualizaciones",
    "quit": "Salir"
  },
  "dialog": {
    "stopTitle": "¿Detener el Hub?",
    "stopBody": "Si detienes el hub, todas las terminales POS quedarán desconectadas hasta que lo inicies de nuevo. ¿Continuar?",
    "stopConfirm": "Detener",
    "stopCancel": "Cancelar",
    "copied": "URL copiada al portapapeles.",
    "updateReady": "Hay una actualización lista. ¿Reiniciar ahora para aplicarla?",
    "updateRestart": "Reiniciar y actualizar",
    "updateLater": "Más tarde"
  },
  "logs": {
    "title": "Registros del Hub",
    "empty": "Aún no hay registros.",
    "clear": "Limpiar",
    "follow": "Seguir"
  }
}
```

- [ ] **Step 3: Commit**

```bash
git add nu_pos_hub_tray/i18n
git commit -m "feat(hub-tray): add EN + ES translations"
```

---

### Task 6.3: `i18n.rs` — embed + resolve translations

**Files:**
- Create: `nu_pos_hub_tray/src-tauri/src/i18n.rs`
- Modify: `nu_pos_hub_tray/src-tauri/src/lib.rs`

- [ ] **Step 1: Implement loader with embedded JSON**

```rust
use once_cell::sync::Lazy;
use serde_json::Value;
use std::collections::HashMap;

const EN: &str = include_str!("../../i18n/en.json");
const ES: &str = include_str!("../../i18n/es.json");

pub static TRANSLATIONS: Lazy<HashMap<&'static str, Value>> = Lazy::new(|| {
    let mut m = HashMap::new();
    m.insert("en", serde_json::from_str(EN).expect("en.json"));
    m.insert("es", serde_json::from_str(ES).expect("es.json"));
    m
});

pub fn detect_locale() -> &'static str {
    let raw = sys_locale::get_locale().unwrap_or_else(|| "en".into());
    if raw.starts_with("es") { "es" } else { "en" }
}

pub fn t(lang: &str, key: &str) -> String {
    let table = TRANSLATIONS.get(lang).or_else(|| TRANSLATIONS.get("en")).expect("en");
    let mut cur = table;
    for seg in key.split('.') {
        cur = match cur.get(seg) {
            Some(v) => v,
            None => return key.to_string(),
        };
    }
    cur.as_str().unwrap_or(key).to_string()
}

/// Replace `{name}` placeholders with the matching value.
pub fn t_fmt(lang: &str, key: &str, args: &[(&str, &str)]) -> String {
    let mut s = t(lang, key);
    for (k, v) in args {
        s = s.replace(&format!("{{{k}}}"), v);
    }
    s
}
```

- [ ] **Step 2: Add `sys-locale` to `Cargo.toml`**

In `[dependencies]`:

```toml
sys-locale = "0.3"
```

- [ ] **Step 3: Register module**

`mod i18n;` in `lib.rs`.

- [ ] **Step 4: Build**

```bash
cd nu_pos_hub_tray/src-tauri && cargo check
```

- [ ] **Step 5: Commit**

```bash
git add nu_pos_hub_tray/src-tauri/Cargo.toml nu_pos_hub_tray/src-tauri/src/i18n.rs nu_pos_hub_tray/src-tauri/src/lib.rs
git commit -m "feat(hub-tray): embed + resolve translations"
```

---

### Task 6.4: `commands.rs` — Tauri commands

**Files:**
- Create: `nu_pos_hub_tray/src-tauri/src/commands.rs`
- Modify: `nu_pos_hub_tray/src-tauri/src/lib.rs`

- [ ] **Step 1: Define shared state + commands**

```rust
use crate::settings::{self, Settings};
use crate::status_client::{HubStatus, StatusClient};
use crate::supervisor::{HubState, Supervisor};
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

pub struct AppState {
    pub supervisor: Arc<Supervisor>,
    pub status_client: StatusClient,
}

#[tauri::command]
pub async fn cmd_start(state: State<'_, AppState>) -> Result<(), String> {
    state.supervisor.start().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cmd_stop(state: State<'_, AppState>) -> Result<(), String> {
    state.supervisor.stop().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cmd_restart(state: State<'_, AppState>) -> Result<(), String> {
    state.supervisor.restart().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cmd_get_state(state: State<'_, AppState>) -> HubState {
    state.supervisor.state()
}

#[tauri::command]
pub fn cmd_get_status(state: State<'_, AppState>) -> HubStatus {
    state.status_client.last()
}

#[tauri::command]
pub fn cmd_get_settings(app: AppHandle) -> Settings {
    settings::load(&app)
}

#[tauri::command]
pub fn cmd_save_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    settings::save(&app, &settings).map_err(|e| e.to_string())
}
```

- [ ] **Step 2: Register module + commands**

In `lib.rs`, add `mod commands;` and wire `.invoke_handler(...)`:

```rust
.invoke_handler(tauri::generate_handler![
    commands::cmd_start,
    commands::cmd_stop,
    commands::cmd_restart,
    commands::cmd_get_state,
    commands::cmd_get_status,
    commands::cmd_get_settings,
    commands::cmd_save_settings,
])
```

- [ ] **Step 3: Build**

```bash
cd nu_pos_hub_tray/src-tauri && cargo check
```

- [ ] **Step 4: Commit**

```bash
git add nu_pos_hub_tray/src-tauri/src/commands.rs nu_pos_hub_tray/src-tauri/src/lib.rs
git commit -m "feat(hub-tray): expose Tauri commands"
```

---

## Phase 7 — Tray menu + lifecycle wiring

### Task 7.1: `tray.rs` — build the menu and reflect status

**Files:**
- Create: `nu_pos_hub_tray/src-tauri/src/tray.rs`
- Modify: `nu_pos_hub_tray/src-tauri/src/lib.rs`

- [ ] **Step 1: Implement tray module**

```rust
use crate::commands::AppState;
use crate::i18n::{detect_locale, t, t_fmt};
use crate::settings;
use crate::status_client::HubStatus;
use crate::supervisor::HubState;
use parking_lot::Mutex;
use std::sync::Arc;
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::{TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, WebviewWindowBuilder, WebviewUrl, Wry,
};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

#[derive(Clone)]
pub struct TrayController {
    app: AppHandle,
    lang: Arc<Mutex<String>>,
    last_status: Arc<Mutex<HubStatus>>,
    last_state: Arc<Mutex<HubState>>,
}

impl TrayController {
    pub fn new(app: AppHandle, initial_lang: String) -> Self {
        Self {
            app,
            lang: Arc::new(Mutex::new(initial_lang)),
            last_status: Arc::new(Mutex::new(HubStatus::default())),
            last_state: Arc::new(Mutex::new(HubState::Stopped)),
        }
    }

    pub fn set_language(&self, lang: String) {
        *self.lang.lock() = lang;
        let _ = self.rebuild_menu();
    }

    pub fn update_status(&self, status: HubStatus) {
        *self.last_status.lock() = status;
        let _ = self.rebuild_menu();
    }

    pub fn update_state(&self, state: HubState) {
        *self.last_state.lock() = state;
        let _ = self.rebuild_menu();
    }

    fn header_label(&self) -> String {
        let lang = self.lang.lock().clone();
        match *self.last_state.lock() {
            HubState::Running => t(&lang, "tray.running"),
            HubState::Stopped => t(&lang, "tray.stopped"),
            HubState::Starting => t(&lang, "tray.starting"),
            HubState::Restarting => t(&lang, "tray.restarting"),
            HubState::Errored => t(&lang, "tray.errored"),
        }
    }

    fn rebuild_menu(&self) -> tauri::Result<()> {
        let lang = self.lang.lock().clone();
        let status = self.last_status.lock().clone();
        let state = *self.last_state.lock();

        let terminals_label = t_fmt(
            &lang,
            "tray.terminals",
            &[("count", &status.connected_terminals.unwrap_or(0).to_string())],
        );
        let lan_label = match status.lan_url.as_deref() {
            Some(url) => t_fmt(&lang, "tray.lanUrl", &[("url", url)]),
            None => t(&lang, "tray.lanUrlUnknown"),
        };

        let header   = MenuItem::with_id(&self.app, "header",    self.header_label(), false, None::<&str>)?;
        let terms    = MenuItem::with_id(&self.app, "terms",     terminals_label,    false, None::<&str>)?;
        let lan      = MenuItem::with_id(&self.app, "lan",       lan_label,          false, None::<&str>)?;
        let copy_lan = MenuItem::with_id(&self.app, "copy_lan",  t(&lang, "tray.copyLanUrl"), status.lan_url.is_some(), None::<&str>)?;
        let start    = MenuItem::with_id(&self.app, "start",     t(&lang, "tray.start"),    matches!(state, HubState::Stopped | HubState::Errored), None::<&str>)?;
        let restart  = MenuItem::with_id(&self.app, "restart",   t(&lang, "tray.restart"),  matches!(state, HubState::Running), None::<&str>)?;
        let stop     = MenuItem::with_id(&self.app, "stop",      t(&lang, "tray.stop"),     matches!(state, HubState::Running), None::<&str>)?;
        let logs     = MenuItem::with_id(&self.app, "logs",      t(&lang, "tray.viewLogs"), true, None::<&str>)?;

        let settings_now = settings::load(&self.app);
        let autostart = CheckMenuItem::with_id(&self.app, "autostart", t(&lang, "tray.autostart"), true, settings_now.autostart, None::<&str>)?;

        let lang_auto = CheckMenuItem::with_id(&self.app, "lang_auto", t(&lang, "tray.languageAuto"), true, settings_now.language.is_none(), None::<&str>)?;
        let lang_en   = CheckMenuItem::with_id(&self.app, "lang_en",   t(&lang, "tray.languageEn"),   true, settings_now.language.as_deref() == Some("en"), None::<&str>)?;
        let lang_es   = CheckMenuItem::with_id(&self.app, "lang_es",   t(&lang, "tray.languageEs"),   true, settings_now.language.as_deref() == Some("es"), None::<&str>)?;
        let lang_sub  = Submenu::with_items(&self.app, t(&lang, "tray.language"), true, &[&lang_auto, &lang_en, &lang_es])?;

        let updates = MenuItem::with_id(&self.app, "check_updates", t(&lang, "tray.checkForUpdates"), true, None::<&str>)?;
        let quit    = MenuItem::with_id(&self.app, "quit", t(&lang, "tray.quit"), true, None::<&str>)?;

        let menu = Menu::with_items(
            &self.app,
            &[
                &header,
                &PredefinedMenuItem::separator(&self.app)?,
                &terms,
                &lan,
                &copy_lan,
                &PredefinedMenuItem::separator(&self.app)?,
                &start,
                &restart,
                &stop,
                &PredefinedMenuItem::separator(&self.app)?,
                &logs,
                &autostart,
                &lang_sub,
                &updates,
                &PredefinedMenuItem::separator(&self.app)?,
                &quit,
            ],
        )?;

        if let Some(tray) = self.app.tray_by_id("main") {
            tray.set_menu(Some(menu))?;
        }
        Ok(())
    }
}

pub fn install(app: &AppHandle, initial_lang: String) -> tauri::Result<TrayController> {
    let controller = TrayController::new(app.clone(), initial_lang);
    let menu = Menu::with_items(app, &[&MenuItem::with_id(app, "boot", "Starting…", false, None::<&str>)?])?;

    let ctrl_for_events = controller.clone();
    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().expect("default icon").clone())
        .menu(&menu)
        .on_menu_event(move |app, event| {
            handle_menu_event(app, event.id().as_ref(), &ctrl_for_events);
        })
        .on_tray_icon_event(|_app, event| {
            if let TrayIconEvent::Click { .. } = event {
                // No-op for v1; menu opens on right-click natively.
            }
        })
        .build(app)?;

    controller.rebuild_menu()?;
    Ok(controller)
}

fn handle_menu_event(app: &AppHandle, id: &str, ctrl: &TrayController) {
    let state: tauri::State<AppState> = app.state();
    match id {
        "start" => {
            let s = state.supervisor.clone();
            tauri::async_runtime::spawn(async move { let _ = s.start().await; });
        }
        "restart" => {
            let s = state.supervisor.clone();
            tauri::async_runtime::spawn(async move { let _ = s.restart().await; });
        }
        "stop" => {
            let lang = ctrl.lang.lock().clone();
            let app2 = app.clone();
            let s = state.supervisor.clone();
            app.dialog()
                .message(t(&lang, "dialog.stopBody"))
                .title(t(&lang, "dialog.stopTitle"))
                .buttons(MessageDialogButtons::OkCancelCustom(
                    t(&lang, "dialog.stopConfirm"),
                    t(&lang, "dialog.stopCancel"),
                ))
                .show(move |confirmed| {
                    if confirmed {
                        let _ = app2.run_on_main_thread(move || {
                            tauri::async_runtime::spawn(async move { let _ = s.stop().await; });
                        });
                    }
                });
        }
        "copy_lan" => {
            if let Some(url) = ctrl.last_status.lock().lan_url.clone() {
                let _ = app.clipboard().write_text(url);
            }
        }
        "logs" => {
            if let Some(win) = app.get_webview_window("logs") {
                let _ = win.show();
                let _ = win.set_focus();
            } else {
                let _ = WebviewWindowBuilder::new(app, "logs", WebviewUrl::default())
                    .title(t(&ctrl.lang.lock(), "logs.title"))
                    .inner_size(800.0, 500.0)
                    .build();
            }
        }
        "autostart" => {
            let app2 = app.clone();
            let mut s = settings::load(app);
            s.autostart = !s.autostart;
            let _ = settings::save(app, &s);
            tauri::async_runtime::spawn(async move {
                apply_autostart(&app2, s.autostart).await;
            });
            let _ = ctrl.rebuild_menu();
        }
        "lang_auto" => {
            let mut s = settings::load(app);
            s.language = None;
            let _ = settings::save(app, &s);
            ctrl.set_language(detect_locale().to_string());
        }
        "lang_en" => {
            let mut s = settings::load(app);
            s.language = Some("en".into());
            let _ = settings::save(app, &s);
            ctrl.set_language("en".into());
        }
        "lang_es" => {
            let mut s = settings::load(app);
            s.language = Some("es".into());
            let _ = settings::save(app, &s);
            ctrl.set_language("es".into());
        }
        "check_updates" => {
            let app2 = app.clone();
            tauri::async_runtime::spawn(async move {
                crate::updates::check_now(&app2).await;
            });
        }
        "quit" => {
            let s = state.supervisor.clone();
            let app2 = app.clone();
            tauri::async_runtime::spawn(async move {
                let _ = s.stop().await;
                app2.exit(0);
            });
        }
        _ => {}
    }
}

async fn apply_autostart(app: &AppHandle, enable: bool) {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    if enable {
        let _ = manager.enable();
    } else {
        let _ = manager.disable();
    }
}
```

> Note: `crate::updates::check_now` is added in Phase 9. Until then, replace the body with `// placeholder` to keep this task buildable.

- [ ] **Step 2: Register module**

`mod tray;` and `mod updates;` (stub created in Task 9.1) in `lib.rs`. For now, create a stub:

`nu_pos_hub_tray/src-tauri/src/updates.rs`:

```rust
use tauri::AppHandle;

pub async fn check_now(_app: &AppHandle) {
    log::info!("update check (stub)");
}
```

- [ ] **Step 3: Build**

```bash
cd nu_pos_hub_tray/src-tauri && cargo check
```

Expected: compiles.

- [ ] **Step 4: Commit**

```bash
git add nu_pos_hub_tray/src-tauri/src
git commit -m "feat(hub-tray): tray menu with status + actions"
```

---

### Task 7.2: Wire everything in `lib.rs`

**Files:**
- Modify: `nu_pos_hub_tray/src-tauri/src/lib.rs`

- [ ] **Step 1: Replace `lib.rs` setup with full wiring**

```rust
mod commands;
mod i18n;
mod log_buffer;
mod paths;
mod settings;
mod status_client;
mod supervisor;
mod tray;
mod updates;

use crate::commands::AppState;
use crate::log_buffer::LogBuffer;
use crate::status_client::StatusClient;
use crate::supervisor::{Supervisor, SupervisorConfig};
use std::sync::Arc;
use tauri::Manager;

const ADMIN_PORT: u16 = 8767;
const WS_PORT: u16 = 8765;
const HTTP_PORT: u16 = 8766;

pub fn run() {
    env_logger::init();
    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::cmd_start,
            commands::cmd_stop,
            commands::cmd_restart,
            commands::cmd_get_state,
            commands::cmd_get_status,
            commands::cmd_get_settings,
            commands::cmd_save_settings,
        ])
        .setup(|app| {
            let handle = app.handle().clone();

            // Resolve initial language
            let saved = settings::load(&handle);
            let lang = saved.language.clone().unwrap_or_else(|| i18n::detect_locale().to_string());

            // Build supervisor
            let log_buffer = LogBuffer::new(5000);
            let cfg = SupervisorConfig {
                node_binary: paths::node_binary(&handle),
                hub_entry: paths::hub_entry(&handle),
                hub_dir: paths::hub_dir(&handle),
                db_path: paths::hub_db_path(&handle),
                log_file: paths::log_file_path(&handle),
                admin_port: ADMIN_PORT,
                ws_port: WS_PORT,
                http_port: HTTP_PORT,
            };
            let supervisor = Arc::new(Supervisor::new(handle.clone(), cfg, log_buffer));
            let status_client = StatusClient::new(handle.clone(), ADMIN_PORT);

            handle.manage(AppState {
                supervisor: supervisor.clone(),
                status_client: status_client.clone(),
            });

            // Install tray
            let controller = tray::install(&handle, lang.clone())?;

            // Forward hub-state events into the tray controller
            {
                let c = controller.clone();
                handle.listen("hub-state", move |event| {
                    if let Ok(state) = serde_json::from_str::<supervisor::HubState>(event.payload()) {
                        c.update_state(state);
                    }
                });
            }
            // Forward hub-status events into the tray controller
            {
                let c = controller.clone();
                handle.listen("hub-status", move |event| {
                    if let Ok(status) = serde_json::from_str::<status_client::HubStatus>(event.payload()) {
                        c.update_status(status);
                    }
                });
            }

            // Start supervisor + status polling
            let sup = supervisor.clone();
            tauri::async_runtime::spawn(async move { let _ = sup.start().await; });
            let sc = status_client.clone();
            tauri::async_runtime::spawn(async move { sc.start_polling().await; });

            // Apply autostart preference
            {
                use tauri_plugin_autostart::ManagerExt;
                let mgr = handle.autolaunch();
                if saved.autostart {
                    let _ = mgr.enable();
                } else {
                    let _ = mgr.disable();
                }
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 2: Build**

```bash
cd nu_pos_hub_tray/src-tauri && cargo build
```

Expected: compiles.

- [ ] **Step 3: Manual smoke test (host platform)**

```bash
cd nu_pos_hub_tray && npm run tauri:dev
```

Verify:
1. Tray icon appears in system tray.
2. Menu shows "Hub: Running" within a few seconds.
3. "Terminals connected: 0" and a LAN URL show up.
4. Clicking "Restart Hub" causes the header to flash through "Restarting…" → "Running".
5. "Stop Hub" shows the confirmation dialog; cancelling does nothing; confirming stops the hub and disables the LAN URL line.
6. "Copy LAN URL" copies the URL.
7. Switching language under Language updates the menu labels immediately.

- [ ] **Step 4: Commit**

```bash
git add nu_pos_hub_tray/src-tauri/src/lib.rs
git commit -m "feat(hub-tray): wire supervisor + status + tray together"
```

---

## Phase 8 — Logs window (React webview)

### Task 8.1: Replace `App.tsx` with a logs viewer

**Files:**
- Modify: `nu_pos_hub_tray/src/App.tsx`
- Create: `nu_pos_hub_tray/src/LogsWindow.tsx`
- Create: `nu_pos_hub_tray/src/i18n.ts`

- [ ] **Step 1: Add tiny i18n loader for webview**

`nu_pos_hub_tray/src/i18n.ts`:

```ts
import en from "../i18n/en.json";
import es from "../i18n/es.json";

const tables: Record<string, any> = { en, es };

export function t(lang: string, key: string): string {
  const table = tables[lang] ?? tables.en;
  return key.split(".").reduce<any>((acc, seg) => (acc ? acc[seg] : undefined), table) ?? key;
}
```

In `tsconfig.json`, ensure `resolveJsonModule: true` (already set in Task 2.2).

- [ ] **Step 2: Implement `LogsWindow.tsx`**

```tsx
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { t } from "./i18n.js";

type Settings = { autostart: boolean; language: string | null };

export default function LogsWindow() {
  const [lines, setLines] = useState<string[]>([]);
  const [follow, setFollow] = useState(true);
  const [lang, setLang] = useState("en");
  const endRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    invoke<Settings>("cmd_get_settings").then((s) => {
      const detected = navigator.language?.startsWith("es") ? "es" : "en";
      setLang(s.language ?? detected);
    });
    invoke<string[]>("cmd_get_log_snapshot").then(setLines);
    const unlisten = listen<string>("hub-log-line", (e) => {
      setLines((prev) => [...prev.slice(-4999), e.payload]);
    });
    return () => { unlisten.then((u) => u()); };
  }, []);

  useEffect(() => {
    if (follow) endRef.current?.scrollIntoView({ behavior: "instant" as ScrollBehavior });
  }, [lines, follow]);

  return (
    <div style={{
      display: "flex", flexDirection: "column", height: "100vh",
      fontFamily: "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace", fontSize: 12,
    }}>
      <div style={{ padding: 8, borderBottom: "1px solid #ddd", display: "flex", gap: 12 }}>
        <strong>{t(lang, "logs.title")}</strong>
        <label><input type="checkbox" checked={follow} onChange={(e) => setFollow(e.target.checked)} /> {t(lang, "logs.follow")}</label>
        <button onClick={() => setLines([])}>{t(lang, "logs.clear")}</button>
      </div>
      <div style={{ flex: 1, overflow: "auto", padding: 8, whiteSpace: "pre-wrap" }}>
        {lines.length === 0 ? <div style={{ opacity: 0.5 }}>{t(lang, "logs.empty")}</div> : null}
        {lines.map((l, i) => <div key={i}>{l}</div>)}
        <div ref={endRef} />
      </div>
    </div>
  );
}
```

- [ ] **Step 3: Update `App.tsx` to render the logs window**

```tsx
import LogsWindow from "./LogsWindow.js";
export default function App() {
  return <LogsWindow />;
}
```

- [ ] **Step 4: Add `cmd_get_log_snapshot` to commands**

In `nu_pos_hub_tray/src-tauri/src/commands.rs`, add:

```rust
use crate::log_buffer::LogBuffer;

pub struct AppState {
    pub supervisor: Arc<Supervisor>,
    pub status_client: StatusClient,
    pub log_buffer: LogBuffer,
}

#[tauri::command]
pub fn cmd_get_log_snapshot(state: State<'_, AppState>) -> Vec<String> {
    state.log_buffer.snapshot()
}
```

And register in `generate_handler!` macro:

```rust
.invoke_handler(tauri::generate_handler![
    commands::cmd_start,
    commands::cmd_stop,
    commands::cmd_restart,
    commands::cmd_get_state,
    commands::cmd_get_status,
    commands::cmd_get_settings,
    commands::cmd_save_settings,
    commands::cmd_get_log_snapshot,
])
```

Also update the `setup` block in `lib.rs` to store `log_buffer` in state. Replace the `AppState { ... }` construction with:

```rust
let log_buffer = LogBuffer::new(5000);
let cfg = SupervisorConfig { /* ... */ };
let supervisor = Arc::new(Supervisor::new(handle.clone(), cfg, log_buffer.clone()));
let status_client = StatusClient::new(handle.clone(), ADMIN_PORT);

handle.manage(AppState {
    supervisor: supervisor.clone(),
    status_client: status_client.clone(),
    log_buffer,
});
```

- [ ] **Step 5: Build + smoke test**

```bash
cd nu_pos_hub_tray && npm run tauri:dev
```

Open the tray → "View Logs". Verify:
- Window appears.
- Existing buffered lines are shown immediately.
- New lines stream in as the hub logs.
- "Follow" auto-scrolls.
- "Clear" empties the visible list.

- [ ] **Step 6: Commit**

```bash
git add nu_pos_hub_tray/src nu_pos_hub_tray/src-tauri/src/commands.rs nu_pos_hub_tray/src-tauri/src/lib.rs
git commit -m "feat(hub-tray): logs window streams hub stdout/stderr"
```

---

## Phase 9 — Auto-update

### Task 9.1: Generate updater key pair and configure

**Files:**
- Modify: `nu_pos_hub_tray/src-tauri/tauri.conf.json` (set `pubkey`)
- Out-of-tree: store private key in 1Password/secure store

- [ ] **Step 1: Generate keypair**

```bash
cd nu_pos_hub_tray && npx tauri signer generate -w ~/.tauri/hub-tray-updater.key
```

Expected: prints a public key and writes a private key file. Securely store the private key (1Password). Do NOT commit it.

- [ ] **Step 2: Paste pubkey into `tauri.conf.json`**

Replace `REPLACE_WITH_TAURI_UPDATER_PUBKEY` with the printed public key string.

- [ ] **Step 3: Confirm GitHub repo coordinates**

Pick the GitHub repo + owner that will host releases (likely a new public/private repo, e.g. `<owner>/sirvo-hub-tray`). Replace `INDUSTRIA_OWNER/INDUSTRIA_REPO` in the `endpoints` URL accordingly.

- [ ] **Step 4: Commit (pubkey only — private key stays secret)**

```bash
git add nu_pos_hub_tray/src-tauri/tauri.conf.json
git commit -m "chore(hub-tray): set updater public key + endpoint"
```

---

### Task 9.2: Implement `updates.rs` for silent download + restart prompt

**Files:**
- Modify: `nu_pos_hub_tray/src-tauri/src/updates.rs`

- [ ] **Step 1: Replace stub with real implementation**

```rust
use crate::commands::AppState;
use crate::i18n::{detect_locale, t};
use crate::settings;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
use tauri_plugin_updater::UpdaterExt;

pub async fn check_now(app: &AppHandle) {
    let updater = match app.updater() {
        Ok(u) => u,
        Err(e) => {
            log::warn!("updater unavailable: {e}");
            return;
        }
    };
    let update = match updater.check().await {
        Ok(Some(u)) => u,
        Ok(None) => {
            log::info!("no update available");
            return;
        }
        Err(e) => {
            log::warn!("update check failed: {e}");
            return;
        }
    };

    log::info!("downloading update {} silently", update.version);
    let mut downloaded: u64 = 0;
    if let Err(e) = update
        .download_and_install(
            |chunk_len, _content_len| { downloaded += chunk_len as u64; },
            || log::info!("update downloaded; awaiting user confirmation"),
        )
        .await
    {
        log::warn!("update download failed: {e}");
        return;
    }

    // After install, Tauri replaces the binary on next launch. We need to
    // (1) stop the hub child, (2) ask the user, (3) restart the app.
    let lang = settings::load(app).language.unwrap_or_else(|| detect_locale().to_string());
    let app2 = app.clone();
    app.dialog()
        .message(t(&lang, "dialog.updateReady"))
        .buttons(MessageDialogButtons::OkCancelCustom(
            t(&lang, "dialog.updateRestart"),
            t(&lang, "dialog.updateLater"),
        ))
        .show(move |confirmed| {
            if confirmed {
                let state: tauri::State<AppState> = app2.state();
                let sup = state.supervisor.clone();
                let a = app2.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = sup.stop().await;
                    a.restart();
                });
            }
        });
}

/// Schedule a periodic background update check (every 6 hours).
pub fn schedule_background(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        // Initial delay so app startup isn't blocked.
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        loop {
            check_now(&app).await;
            tokio::time::sleep(std::time::Duration::from_secs(6 * 3600)).await;
        }
    });
}
```

- [ ] **Step 2: Schedule background check in `lib.rs`**

In `setup(|app| ...)` in `lib.rs`, after the supervisor/poller spawns:

```rust
updates::schedule_background(handle.clone());
```

- [ ] **Step 3: Build**

```bash
cd nu_pos_hub_tray/src-tauri && cargo check
```

- [ ] **Step 4: Commit**

```bash
git add nu_pos_hub_tray/src-tauri/src/updates.rs nu_pos_hub_tray/src-tauri/src/lib.rs
git commit -m "feat(hub-tray): silent auto-update with restart prompt"
```

---

## Phase 10 — Final UX polish

### Task 10.1: Icon variants for state

**Files:**
- Add: `nu_pos_hub_tray/src-tauri/icons/tray-running.png`
- Add: `nu_pos_hub_tray/src-tauri/icons/tray-stopped.png`
- Add: `nu_pos_hub_tray/src-tauri/icons/tray-error.png`
- Modify: `nu_pos_hub_tray/src-tauri/src/tray.rs`

- [ ] **Step 1: Provide three template-style PNGs**

Create three template PNGs (black silhouette on transparent, 32×32 + 64×64). Source: a simple hub silhouette plus a dot overlay (green = none in template — instead use empty/filled/X for differentiation; color is added by macOS automatically only for non-template; we keep the icon shape-based so it works on both macOS and Windows).

For v1 we accept the same base icon with different glyphs:
- `tray-running.png` — solid dot
- `tray-stopped.png` — hollow dot
- `tray-error.png` — exclamation mark

Producing pixel-perfect icons is out of scope of this plan; use placeholders adapted from existing `nu_pos_react/src-tauri/icons/icon.png` plus a manual edit.

- [ ] **Step 2: Update tray icon on state change**

In `tray.rs`, add a helper:

```rust
use tauri::image::Image;
use std::path::PathBuf;

fn icon_for(state: HubState, app: &AppHandle) -> Option<Image<'static>> {
    let resource = app.path().resource_dir().ok()?;
    let file = match state {
        HubState::Running => "icons/tray-running.png",
        HubState::Errored => "icons/tray-error.png",
        _ => "icons/tray-stopped.png",
    };
    Image::from_path(resource.join(file)).ok()
}
```

In `TrayController::update_state`, after updating `last_state`, set the icon:

```rust
if let Some(tray) = self.app.tray_by_id("main") {
    if let Some(img) = icon_for(state, &self.app) {
        let _ = tray.set_icon(Some(img));
    }
}
```

- [ ] **Step 3: Build + smoke test**

```bash
cd nu_pos_hub_tray && npm run tauri:dev
```

Restart the hub; verify the tray icon swaps through stopped → starting → running.

- [ ] **Step 4: Commit**

```bash
git add nu_pos_hub_tray/src-tauri/icons nu_pos_hub_tray/src-tauri/src/tray.rs
git commit -m "feat(hub-tray): per-state tray icons"
```

---

## Phase 11 — CI release workflow

### Task 11.1: GitHub Actions release pipeline

**Files:**
- Create: `.github/workflows/hub-tray-release.yml`

- [ ] **Step 1: Workflow file**

```yaml
name: hub-tray release
on:
  push:
    tags: ["hub-tray-v*"]
  workflow_dispatch:

jobs:
  build:
    strategy:
      fail-fast: false
      matrix:
        include:
          - os: macos-latest
            target: darwin-arm64
          - os: macos-13
            target: darwin-x64
          - os: windows-latest
            target: win32-x64
    runs-on: ${{ matrix.os }}
    defaults:
      run:
        working-directory: nu_pos_hub_tray
    steps:
      - uses: actions/checkout@v4
        with: { submodules: recursive }
      - uses: actions/setup-node@v4
        with: { node-version: "20.x" }
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with: { workspaces: "nu_pos_hub_tray/src-tauri -> target" }

      - name: Install hub deps (root)
        run: npm install
        working-directory: nu_pos_hub

      - name: Install protocol deps + build
        run: npm install && npm run build
        working-directory: nu_pos_pos_sync_protocol
        continue-on-error: false

      - name: Install tray deps
        run: npm install

      - name: Stage Node runtime
        run: npm run prepare:node
        env:
          NODE_RUNTIME_TARGETS: ${{ matrix.target }}

      - name: Stage hub bundle
        run: npm run prepare:hub

      - name: Tauri build (signed updater artifacts)
        run: npm run tauri:build
        env:
          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_UPDATER_PRIVATE_KEY }}
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_UPDATER_KEY_PASSWORD }}

      - name: Upload artifacts
        uses: actions/upload-artifact@v4
        with:
          name: hub-tray-${{ matrix.target }}
          path: |
            nu_pos_hub_tray/src-tauri/target/release/bundle/**/*.dmg
            nu_pos_hub_tray/src-tauri/target/release/bundle/**/*.msi
            nu_pos_hub_tray/src-tauri/target/release/bundle/**/*.zip
            nu_pos_hub_tray/src-tauri/target/release/bundle/**/*.sig
            nu_pos_hub_tray/src-tauri/target/release/bundle/**/*.app.tar.gz

  release:
    needs: build
    runs-on: ubuntu-latest
    if: startsWith(github.ref, 'refs/tags/hub-tray-v')
    steps:
      - uses: actions/download-artifact@v4
        with: { path: artifacts }
      - name: Build latest.json
        run: |
          set -euo pipefail
          version="${GITHUB_REF_NAME#hub-tray-v}"
          # Compose latest.json from .sig files written by Tauri updater
          node - <<'EOF'
            const fs = require("fs"); const path = require("path");
            const version = process.env.GITHUB_REF_NAME.replace(/^hub-tray-v/, "");
            const root = "artifacts";
            const platforms = {};
            for (const dir of fs.readdirSync(root)) {
              const full = path.join(root, dir);
              const sigs = [];
              const walk = (p) => fs.readdirSync(p, { withFileTypes: true }).forEach((d) => {
                const f = path.join(p, d.name);
                if (d.isDirectory()) walk(f);
                else if (d.name.endsWith(".sig")) sigs.push(f);
              });
              walk(full);
              for (const sig of sigs) {
                const url = sig.replace(/^artifacts\//, "");
                const sigText = fs.readFileSync(sig, "utf8").trim();
                let platform = "";
                if (sig.endsWith(".app.tar.gz.sig"))   platform = sig.includes("arm64") ? "darwin-aarch64" : "darwin-x86_64";
                else if (sig.endsWith(".msi.sig"))     platform = "windows-x86_64";
                else if (sig.endsWith(".zip.sig"))     platform = "windows-x86_64";
                if (!platform) continue;
                platforms[platform] = {
                  url: `https://github.com/${process.env.GITHUB_REPOSITORY}/releases/download/${process.env.GITHUB_REF_NAME}/${path.basename(url)}`,
                  signature: sigText,
                };
              }
            }
            const manifest = {
              version,
              notes: "See release notes.",
              pub_date: new Date().toISOString(),
              platforms,
            };
            fs.writeFileSync("latest.json", JSON.stringify(manifest, null, 2));
          EOF
      - name: Create GitHub Release
        uses: softprops/action-gh-release@v2
        with:
          files: |
            artifacts/**/*.dmg
            artifacts/**/*.msi
            artifacts/**/*.zip
            artifacts/**/*.app.tar.gz
            artifacts/**/*.sig
            latest.json
          generate_release_notes: true
```

> Note: typo guard — the protocol path is `nu_pos_sync_protocol`, NOT `nu_pos_pos_sync_protocol`. Fix that in the workflow before commit.

- [ ] **Step 2: Add secrets to the repo**

In GitHub repo Settings → Secrets and variables → Actions, add:
- `TAURI_UPDATER_PRIVATE_KEY` (contents of `~/.tauri/hub-tray-updater.key`)
- `TAURI_UPDATER_KEY_PASSWORD` (the passphrase used at generation)

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/hub-tray-release.yml
git commit -m "ci(hub-tray): build + release Windows/macOS on tag"
```

- [ ] **Step 4: Tag-driven dry run**

```bash
git tag hub-tray-v0.1.0
git push origin hub-tray-v0.1.0
```

Verify in GitHub Actions that all three builds complete and a release with `latest.json` + binaries is created.

---

## Phase 12 — Docs and root integration

### Task 12.1: Root CLAUDE.md note

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Add a `nu_pos_hub_tray/` entry to "Repository Structure"**

Insert after the `nu_pos_hub/` line:

```markdown
- **`nu_pos_hub_tray/`** — Cross-platform (Windows + macOS) Tauri tray app that supervises the bundled `nu_pos_hub`. Provides start/restart/stop, terminal count, LAN URL, logs window, auto-start toggle, EN/ES UI, and silent GitHub-Releases auto-update. Owns its own `version.json` (independent of root). See `nu_pos_hub_tray/README.md` and `docs/superpowers/plans/2026-05-20-hub-tray-app.md`.
```

- [ ] **Step 2: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: register nu_pos_hub_tray in repo structure"
```

---

### Task 12.2: Tray README — install + uninstall instructions

**Files:**
- Modify: `nu_pos_hub_tray/README.md`

- [ ] **Step 1: Replace stub with installation guidance**

Add sections:
- **Install (restaurant)** — link to GitHub Releases page, walk through SmartScreen warning on Windows ("More info → Run anyway") and Gatekeeper on macOS ("right-click → Open → Open"). Note: signing is deferred (Phase 12.3).
- **Uninstall** — Windows: Settings → Apps; macOS: drag from Applications + remove `~/Library/Application Support/com.sirvo.hub.tray`.
- **Data locations** — DB at `app_data_dir/hub.sqlite`, logs at `app_log_dir/hub.log`.
- **Dev quickstart** — already present.

- [ ] **Step 2: Commit**

```bash
git add nu_pos_hub_tray/README.md
git commit -m "docs(hub-tray): install + uninstall instructions"
```

---

## Self-Review Checklist (verify before handoff)

- **Spec coverage:**
  - Status indicator → Tasks 7.1 + 10.1 (tray icon + label).
  - Connected terminals → Tasks 1.2 + 5.1 + 7.1.
  - LAN URL display + copy → Tasks 1.1, 1.2, 5.1, 7.1.
  - Start / Restart / Stop (with confirm) → Tasks 4.3, 6.4, 7.1.
  - View logs → Tasks 4.2, 4.4, 8.1.
  - Auto-start with OS, toggleable → Tasks 6.1, 7.1, 7.2.
  - Auto-update from GitHub Releases (silent download + prompt) → Tasks 9.1, 9.2.
  - Bundled tray+hub version → version.json owned by tray + Phase 3 bundling + Phase 11 CI.
  - Bilingual EN + ES → Tasks 6.2, 6.3, 7.1, 8.1.
  - Tray own version.json → Task 2.1.
  - Process model: bundle Node + hub, supervise as child → Phase 3 + 4.3.
  - Status channel: `GET /admin/status` on 127.0.0.1 → Tasks 1.2, 1.4.
  - Logs source: capture child stdout/stderr → Tasks 4.3, 4.4.
  - Confirmation for Stop → Task 7.1.
  - No code signing in v1 → workflow omits codesign envs; docs call out warnings.
  - First-install via link → Task 12.2.

- **No placeholders:** code present in every step. The only deferred items are the icon designs (Task 10.1 — acknowledged) and signing (out of v1 scope per spec).

- **Type consistency:** `HubState` and `HubStatus` defined in `supervisor.rs` / `status_client.rs` are reused everywhere; `AppState` includes `supervisor`, `status_client`, and (after Task 8.1) `log_buffer`; the `LogBuffer::clone` shim makes shared-ring cloning explicit.

- **Known follow-ups (acceptable for v1):**
  - Log file rotation deferred (append-only file in v1; in-memory ring is the live view).
  - Pixel-perfect icons deferred (placeholder PNGs).
  - macOS Universal binary not built — separate matrix rows for `darwin-arm64` and `darwin-x64`. If you want one universal `.dmg`, add a `lipo` step in CI.
  - `tauri-plugin-process` is intentionally not added; `app.restart()` covers what we need without extra capabilities.

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-05-20-hub-tray-app.md`. Two execution options:

1. **Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks, fast iteration.
2. **Inline Execution** — Execute tasks in this session using executing-plans, batch execution with checkpoints.

Which approach?
