#!/usr/bin/env node
// Preinstall step: replace the placeholder `bin/pacquet` with the platform's
// native binary so the `pacquet` command runs the binary directly, with no
// Node.js launcher in the way.
//
// `bin/pacquet` is published as a non-executable placeholder rather than a Node
// launcher on purpose. A launcher would force every `pacquet` invocation
// through Node startup (~170ms) just to spawn the ~30ms binary, and on Windows
// the `.bin` shim generated from its `#!/usr/bin/env node` shebang would
// hardcode a `node bin/pacquet` call this script could not undo (npm does not
// re-read package.json after preinstall). So the binary has to be in place
// before the command is ever resolved.
//
// Consequence: when this script does not run — `--ignore-scripts`, or pnpm/Bun
// blocking build scripts until `pacquet` is allow-listed — `bin/pacquet` stays
// a placeholder and the command will not work until it does. This mirrors how
// `@pnpm/exe` ships pnpm's native binary.
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

setup();

function setup() {
  const candidates = getBinCandidates();
  if (candidates.length === 0) {
    fail(`pacquet does not ship a prebuilt binary for ${platform}-${arch}.`);
  }

  // Resolve whichever candidate the package manager actually installed: it
  // already filtered the `@pacquet/*` packages by their `os`/`cpu`/`libc`
  // fields, so trusting that is more reliable than re-deriving the platform.
  // `getBinCandidates` orders the linux glibc/musl pair by detected libc, which
  // only matters when both are present (e.g. `npm install --force` installs
  // every optional dependency regardless of those fields).
  let nativeBinary;
  for (const target of candidates) {
    try {
      nativeBinary = require.resolve(target);
      break;
    } catch {}
  }
  if (nativeBinary == null) {
    const pkgName = candidates[0].split("/").slice(0, 2).join("/");
    fail(
      `The "${pkgName}" package is not installed, so pacquet has no native binary to run.\n` +
      "If your package manager skipped optional dependencies or blocked build scripts, " +
      "enable them and reinstall."
    );
  }

  const binDir = path.join(__dirname, "bin");
  if (platform === "win32") {
    // The `.bin` shim already points at the original `bin/pacquet` name and npm
    // won't re-read package.json, so the file at that name must be the binary.
    // Also drop a `.exe` twin and repoint `bin` at it, so any shim generated
    // from here on (npm's own linking, a later cmd-shim regeneration) targets
    // the executable directly.
    placeBinary(nativeBinary, path.join(binDir, "pacquet.exe"));
    placeBinary(nativeBinary, path.join(binDir, "pacquet"));
    rewriteBin("bin/pacquet.exe");
  } else {
    // 0o755: the swapped file is what the `.bin/pacquet` entry resolves to.
    placeBinary(nativeBinary, path.join(binDir, "pacquet"), 0o755);
  }
}

/**
 * Atomically places `nativeBinary` at `destPath` via a temp file + rename, so a
 * concurrent invocation never sees a half-written file. Hard-links first (no
 * second copy of the ~13MB binary on disk) and falls back to a copy across
 * filesystems. Exits the process on failure — without the binary there is no
 * working `pacquet`.
 *
 * @param {string} nativeBinary Absolute path to the resolved native binary.
 * @param {string} destPath Absolute path to create.
 * @param {number} [mode] chmod to apply to a copy-created file. Skipped for hard
 *   links — they share the source inode, which under pnpm is the shared store
 *   blob other projects link to.
 */
function placeBinary(nativeBinary, destPath, mode) {
  const tempPath = `${destPath}.pacquet-tmp`;
  try {
    fs.rmSync(tempPath, { force: true });
    let linked = false;
    try {
      fs.linkSync(nativeBinary, tempPath);
      linked = true;
    } catch {
      fs.copyFileSync(nativeBinary, tempPath);
    }
    if (!linked && mode != null) {
      fs.chmodSync(tempPath, mode);
    }
    fs.renameSync(tempPath, destPath);
  } catch (err) {
    try {
      fs.rmSync(tempPath, { force: true });
    } catch {}
    fail(`Could not install the pacquet binary at ${destPath}: ${err.message}`);
  }
}

function rewriteBin(binValue) {
  const pkgJsonPath = path.join(__dirname, "package.json");
  // Non-fatal: the `.exe` twin and the binary at the original name already make
  // `pacquet` runnable; the rewrite only helps shims regenerated later.
  try {
    const pkg = JSON.parse(fs.readFileSync(pkgJsonPath, "utf8"));
    pkg.bin = binValue;
    fs.writeFileSync(pkgJsonPath, JSON.stringify(pkg, null, 2));
  } catch {}
}

function fail(message) {
  console.error(message);
  process.exit(1);
}

/**
 * Native binary specifiers to try, most-preferred first. Empty when the host
 * platform/arch is unsupported. The linux glibc/musl pair is ordered by the
 * detected libc; the caller resolves whichever is installed, so the order only
 * decides the winner when both happen to be present.
 *
 * @returns {string[]}
 */
function getBinCandidates() {
  const platformEntry = PLATFORMS?.[platform]?.[arch];

  if (platformEntry == null) {
    return [];
  }
  if (typeof platformEntry === "string") {
    return [platformEntry];
  }

  const order = detectLinuxLibc() === "musl" ? ["musl", "glibc"] : ["glibc", "musl"];
  return order.map((libc) => platformEntry[libc]);
}

function detectLinuxLibc() {
  if (platform !== "linux") {
    return null;
  }

  // Node sets `glibcVersionRuntime` to the glibc it links against, and leaves it
  // unset on musl. Guarded because `process.report` can be unavailable in some
  // runtimes — when it is, fall back to the default ordering and let resolution
  // pick the installed package.
  try {
    return process.report?.getReport().header.glibcVersionRuntime ? "glibc" : "musl";
  } catch {
    return null;
  }
}
