#!/usr/bin/env node
/**
 * Downloads Litestream for the build platform, checks it against the pinned
 * SHA-256 and stages it into src-tauri/resources/litestream/.
 *
 * The supervisor runs the hub under `litestream replicate` when hub.env sets
 * LITESTREAM_REPLICA_URL (see src-tauri/src/backup.rs). Only Windows bundles it
 * for now; other targets get a placeholder so the bundle's resource glob still
 * matches, and the hub there reports backup mode "unavailable".
 *
 * Honors LITESTREAM_TARGETS (comma-separated, e.g. "win32-x64"); default:
 * current host. A checksum mismatch fails the build.
 */
import { mkdirSync, existsSync, createWriteStream, readFileSync, writeFileSync, rmSync, renameSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import https from "node:https";

const __dirname = dirname(fileURLToPath(import.meta.url));
const stagingRoot = resolve(__dirname, "..", "src-tauri/resources/litestream");

const VERSION = "0.5.17";

// From the release's checksums.txt, pinned here so a replaced asset fails the
// build instead of shipping.
const platforms = {
  "win32-x64": {
    archive: `litestream-${VERSION}-windows-x86_64.zip`,
    sha256: "6f5532687d71c452e0099a9087e51da65f9935664fb11549abc0a5a34cb1d34e",
    binary: "litestream.exe",
  },
};

function targets() {
  const explicit = process.env.LITESTREAM_TARGETS;
  if (explicit) return explicit.split(",").map((s) => s.trim()).filter(Boolean);
  return [`${process.platform}-${process.arch}`];
}

function download(url, dest) {
  return new Promise((resolveP, rejectP) => {
    https.get(url, (res) => {
      if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
        res.resume();
        return download(res.headers.location, dest).then(resolveP, rejectP);
      }
      if (res.statusCode !== 200) {
        res.resume();
        return rejectP(new Error(`Download failed: ${res.statusCode} for ${url}`));
      }
      const file = createWriteStream(dest);
      res.pipe(file);
      file.on("finish", () => file.close(resolveP));
      file.on("error", rejectP);
    }).on("error", rejectP);
  });
}

function sha256(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

async function stage(target) {
  const spec = platforms[target];
  mkdirSync(stagingRoot, { recursive: true });
  if (!spec) {
    writeFileSync(
      resolve(stagingRoot, "README.txt"),
      `Litestream ${VERSION} is bundled with the Windows build only.\n`,
    );
    console.log(`[litestream] ${target}: not bundled (placeholder only)`);
    return;
  }
  const binaryPath = resolve(stagingRoot, spec.binary);
  const stamp = resolve(stagingRoot, "VERSION");
  if (existsSync(binaryPath) && existsSync(stamp) && readFileSync(stamp, "utf8").trim() === VERSION) {
    console.log(`[litestream] ${target}: ${VERSION} already staged`);
    return;
  }

  const url = `https://github.com/benbjohnson/litestream/releases/download/v${VERSION}/${spec.archive}`;
  const archivePath = resolve(stagingRoot, spec.archive);
  console.log(`[litestream] downloading ${url}`);
  await download(url, archivePath);
  const actual = sha256(archivePath);
  if (actual !== spec.sha256) {
    rmSync(archivePath, { force: true });
    throw new Error(`[litestream] checksum mismatch for ${spec.archive}: expected ${spec.sha256}, got ${actual}`);
  }
  console.log(`[litestream] checksum OK (${actual})`);

  const extractDir = resolve(stagingRoot, "_extract");
  rmSync(extractDir, { recursive: true, force: true });
  mkdirSync(extractDir, { recursive: true });
  execFileSync("unzip", ["-q", archivePath, "-d", extractDir]);
  rmSync(binaryPath, { force: true });
  renameSync(resolve(extractDir, spec.binary), binaryPath);
  // Apache-2.0: ship its license next to the binary.
  renameSync(resolve(extractDir, "LICENSE"), resolve(stagingRoot, "LICENSE"));
  rmSync(extractDir, { recursive: true, force: true });
  rmSync(archivePath, { force: true });
  writeFileSync(stamp, `${VERSION}\n`);
  console.log(`[litestream] staged ${target} -> ${binaryPath}`);
}

for (const t of targets()) {
  await stage(t);
}
