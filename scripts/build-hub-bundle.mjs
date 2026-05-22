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
// Hub + sync protocol are pulled in via the git submodule at external/source
// (a sparse clone of josespinal/rost_pos_restaurant). Locally, you can also
// `cd external/source && git pull` to refresh against the monorepo's main.
const sourceRoot = resolve(trayRoot, "external/source");
const hubRoot = resolve(sourceRoot, "nu_pos_hub");
const stagingRoot = resolve(trayRoot, "src-tauri/resources/hub");

function sh(cmd, args, cwd) {
  console.log(`$ ${cmd} ${args.join(" ")}  (cwd=${cwd})`);
  // shell:true on Windows so PATHEXT resolves .cmd shims (npm, tar, unzip).
  execFileSync(cmd, args, { cwd, stdio: "inherit", shell: process.platform === "win32" });
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
// Pin transitive versions that the hub was tested against. bonjour-service
// 1.4.0 dropped the `Bonjour` named export and the hub imports it that way;
// the hub's lockfile resolves it to 1.3.0. Pin the direct dep to the exact
// version so a fresh `npm install` in staging matches the tested resolution.
if (pkg.dependencies?.["bonjour-service"]) {
  pkg.dependencies["bonjour-service"] = "1.3.0";
}
// Replace file: deps with their resolved counterparts during install.
writeFileSync(resolve(stagingRoot, "package.json"), JSON.stringify(pkg, null, 2));

console.log("[hub] installing production deps in staging");
// nu_pos_hub depends on @nu/sync-protocol via file:../nu_pos_sync_protocol.
// We need that to be installable from the staging dir. Copy it in.
const protoSrc = resolve(sourceRoot, "nu_pos_sync_protocol");
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
