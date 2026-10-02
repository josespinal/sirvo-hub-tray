#!/usr/bin/env node
/**
 * Walks the downloaded CI artifacts to build a Tauri updater latest.json.
 * Inputs (env):
 *   GITHUB_REF_NAME       e.g. "hub-tray-v0.1.0"
 *   GITHUB_REPOSITORY     e.g. "josespinal/sirvo-hub-tray"
 *   ARTIFACTS_DIR         path to the artifacts root (default "../artifacts")
 */
import { readdirSync, readFileSync, writeFileSync, statSync } from "node:fs";
import { join, basename } from "node:path";

const refName = process.env.GITHUB_REF_NAME;
const repo = process.env.GITHUB_REPOSITORY;
const artifactsDir = process.env.ARTIFACTS_DIR || "../artifacts";

if (!refName || !repo) {
  throw new Error("GITHUB_REF_NAME and GITHUB_REPOSITORY are required");
}

// Tags are "v1.2.3" (older ones "hub-tray-v1.2.3"); latest.json wants plain semver.
const version = refName.replace(/^(hub-tray-)?v/, "");

function walk(dir, out = []) {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) walk(full, out);
    else out.push(full);
  }
  return out;
}

const sigs = walk(artifactsDir).filter((p) => p.endsWith(".sig"));
const platforms = {};

for (const sig of sigs) {
  let platform = "";
  if (sig.endsWith(".app.tar.gz.sig")) {
    platform = sig.includes("arm64") ? "darwin-aarch64" : "darwin-x86_64";
  } else if (sig.endsWith(".msi.sig") || sig.endsWith(".zip.sig")) {
    platform = "windows-x86_64";
  }
  if (!platform) continue;
  const signature = readFileSync(sig, "utf8").trim();
  platforms[platform] = {
    // GitHub stores release assets with spaces turned into dots
    // ("Sirvo Hub.app.tar.gz" -> "Sirvo.Hub.app.tar.gz"); link the stored name.
    url: `https://github.com/${repo}/releases/download/${refName}/${basename(sig.replace(/\.sig$/, "")).replace(/ /g, ".")}`,
    signature,
  };
}

const manifest = {
  version,
  notes: `See release ${refName}`,
  pub_date: new Date().toISOString(),
  platforms,
};

writeFileSync("latest.json", JSON.stringify(manifest, null, 2));
console.log("wrote latest.json with platforms: " + Object.keys(platforms).join(", "));
