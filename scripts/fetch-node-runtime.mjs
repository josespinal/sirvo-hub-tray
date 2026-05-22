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
