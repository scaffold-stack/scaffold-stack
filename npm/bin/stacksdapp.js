#!/usr/bin/env node
"use strict";

const path = require("node:path");
const fs = require("node:fs");
const os = require("node:os");
const { spawn } = require("node:child_process");
const { pipeline } = require("node:stream/promises");

// Reads this wrapper's version so we can download the matching Rust binary from releases.
const wrapperPkg = require("../package.json");

const CLI_NAME = "stacksdapp";
const VERSION = wrapperPkg.version;

function detectTarget() {
  const platform = process.platform;
  const arch = process.arch;

  if (platform === "darwin" && arch === "x64") return "x86_64-apple-darwin";
  if (platform === "darwin" && arch === "arm64") return "aarch64-apple-darwin";
  if (platform === "linux" && arch === "x64") return "x86_64-unknown-linux-gnu";
  if (platform === "win32" && arch === "x64") return "x86_64-pc-windows-msvc";

  throw new Error(`Unsupported platform/arch: ${platform} ${arch}`);
}

function getExt() {
  return process.platform === "win32" ? ".exe" : "";
}

function defaultBinaryUrl(target, ext) {
  const githubRepo = process.env.STACKSDAPP_GITHUB_REPO || "scaffold-stack/scaffold-stack";
  const tag = "v" + VERSION;
  const assetName = `${CLI_NAME}-${target}${ext}`;
  return `https://github.com/${githubRepo}/releases/download/${tag}/${assetName}`;
}

async function downloadToFile(url, destPath) {
  const res = await fetch(url);
  if (!res.ok) {
    throw new Error(`Failed to download binary (${res.status}): ${url}`);
  }

  // Stream straight to disk to avoid loading the whole binary in memory.
  await pipeline(res.body, fs.createWriteStream(destPath));
}

async function main() {
  // Optional escape hatch for power users / CI:
  // STACKSDAPP_BINARY_PATH=/path/to/stacksdapp stacksdapp --help
  const binaryFromEnv = process.env.STACKSDAPP_BINARY_PATH;
  if (binaryFromEnv) {
    const child = spawn(binaryFromEnv, process.argv.slice(2), { stdio: "inherit" });
    child.on("exit", (code) => process.exit(code == null ? 1 : code));
    return;
  }

  const target = detectTarget();
  const ext = getExt();

  const cacheDir = path.join(os.homedir(), ".cache", CLI_NAME, VERSION, target);
  fs.mkdirSync(cacheDir, { recursive: true });

  const binaryPath = path.join(cacheDir, `${CLI_NAME}${ext}`);

  const binaryBaseUrl = process.env.STACKSDAPP_BINARY_BASE_URL;
  const url =
    process.env.STACKSDAPP_BINARY_URL ||
    (binaryBaseUrl
      ? `${binaryBaseUrl}/v${VERSION}/${CLI_NAME}-${target}${ext}`
      : defaultBinaryUrl(target, ext));

  if (!fs.existsSync(binaryPath)) {
    process.stderr.write(`Downloading ${CLI_NAME} ${VERSION} (${target})...\n`);
    await downloadToFile(url, binaryPath);
  }

  // Ensure the cached binary is executable (especially after upgrades/cross-filesystem copies).
  if (process.platform !== "win32") {
    fs.chmodSync(binaryPath, 0o755);
  }

  const child = spawn(binaryPath, process.argv.slice(2), {
    stdio: "inherit",
    windowsHide: true,
  });

  child.on("exit", (code) => process.exit(code == null ? 1 : code));
  child.on("error", (err) => {
    console.error(`Failed to run ${CLI_NAME}: ${err?.message || String(err)}`);
    process.exit(1);
  });
}

main().catch((err) => {
  console.error(String(err?.message || err));
  process.exit(1);
});

