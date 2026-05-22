#!/usr/bin/env node
/**
 * Stamps tauri.conf.json + Cargo.toml from version.json at repo root.
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
