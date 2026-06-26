#!/usr/bin/env node
// Preinstall optimization. The published `bin/pacquet` is a Node.js launcher
// shim, so every invocation otherwise pays full Node startup (~170ms) just to
// spawn the real native binary. Here we overwrite that shim file in place with
// the platform's native binary, so the `.bin/pacquet` entry the package manager
// already created resolves straight to the binary and no Node process is
// started.
//
// This is best-effort. When it can't run or can't apply (script blocked by
// `--ignore-scripts` or pnpm's build gate, Windows, Yarn PnP, unsupported
// platform, or any I/O error) the original JS shim stays in place and keeps
// working — just slower.
const fs = require("fs");
const path = require("path");
const { platform, arch } = process;

const PLATFORMS = {
  win32: {
    x64: "@pacquet/win32-x64/pacquet.exe",
    arm64: "@pacquet/win32-arm64/pacquet.exe",
  },
  darwin: {
    x64: "@pacquet/darwin-x64/pacquet",
    arm64: "@pacquet/darwin-arm64/pacquet",
  },
  linux: {
    x64: {
      glibc: "@pacquet/linux-x64/pacquet",
      musl: "@pacquet/linux-x64-musl/pacquet",
    },
    arm64: {
      glibc: "@pacquet/linux-arm64/pacquet",
      musl: "@pacquet/linux-arm64-musl/pacquet",
    },
  },
};

optimize();

function optimize() {
  // The `.bin` entry on Windows is a generated `.cmd`/`.ps1` shim that invokes
  // the shebang target through Node, so swapping the shim file for a binary
  // wouldn't bypass Node. Leave the JS launcher in place there.
  if (platform === "win32") {
    return;
  }

  // Under Yarn Plug'n'Play there is no real `.bin` symlink pointing at this
  // file, so there is nothing to relink.
  if (process.versions.pnp != null) {
    return;
  }

  const target = getBinPath();
  if (target == null) {
    return;
  }

  let nativeBinary;
  try {
    nativeBinary = require.resolve(target);
  } catch {
    // The platform package isn't installed (e.g. optional deps were skipped).
    return;
  }

  const shimPath = path.join(__dirname, "bin", "pacquet");
  const tempPath = `${shimPath}.pacquet-tmp`;
  try {
    fs.rmSync(tempPath, { force: true });
    try {
      // A hard link avoids a second copy of the ~13MB binary on disk.
      fs.linkSync(nativeBinary, tempPath);
    } catch {
      // Hard links can't cross filesystems; fall back to a copy.
      fs.copyFileSync(nativeBinary, tempPath);
    }
    fs.chmodSync(tempPath, 0o755);
    // Atomic swap so a concurrent invocation never sees a half-written file.
    fs.renameSync(tempPath, shimPath);
  } catch {
    try {
      fs.rmSync(tempPath, { force: true });
    } catch {}
  }
}

function getBinPath() {
  const platformEntry = PLATFORMS?.[platform]?.[arch];

  if (platformEntry == null || typeof platformEntry === "string") {
    return platformEntry;
  }

  return platformEntry[detectLinuxLibc()];
}

function detectLinuxLibc() {
  if (platform !== "linux") {
    return null;
  }

  return process.report.getReport().header.glibcVersionRuntime ? "glibc" : "musl";
}
