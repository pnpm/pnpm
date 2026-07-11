#!/usr/bin/env node
// Preinstall: replace the placeholder `bin/pnpr` with the platform's native
// binary, so the command runs the binary directly instead of paying Node.js
// startup on every call. Mirrors how `@pnpm/exe` ships pnpm.
//
// The published `bin/pnpr` is a shebang-less placeholder: the Windows `.bin`
// shim is generated from the bin file, so a Node launcher there would bake in a
// `node bin/pnpr` call this script cannot rewrite (npm does not re-read
// package.json after preinstall). The cost is that there is no fallback — when
// build scripts are blocked (`--ignore-scripts`, pnpm/Bun defaults) the
// placeholder stays until the build is allow-listed.
import console from 'node:console'
import fs from 'node:fs'
import { createRequire } from 'node:module'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const require = createRequire(import.meta.url)
const ownDir = path.dirname(fileURLToPath(import.meta.url))
const { platform, arch } = process

const PLATFORMS = {
  win32: {
    x64: '@pnpm/pnpr.win32-x64/pnpr.exe',
    arm64: '@pnpm/pnpr.win32-arm64/pnpr.exe',
  },
  darwin: {
    x64: '@pnpm/pnpr.darwin-x64/pnpr',
    arm64: '@pnpm/pnpr.darwin-arm64/pnpr',
  },
  linux: {
    x64: {
      glibc: '@pnpm/pnpr.linux-x64/pnpr',
      musl: '@pnpm/pnpr.linux-x64-musl/pnpr',
    },
    arm64: {
      glibc: '@pnpm/pnpr.linux-arm64/pnpr',
      musl: '@pnpm/pnpr.linux-arm64-musl/pnpr',
    },
  },
}

setup()

function setup () {
  // The committed manifest has no `optionalDependencies`; generate-packages.mjs
  // adds them at release time. Without them this is the monorepo checkout, where
  // the wrapper is a workspace package and there is no native binary to link.
  if (readOwnManifest().optionalDependencies == null) {
    return
  }

  const candidates = getBinCandidates()
  if (candidates.length === 0) {
    fail(`@pnpm/pnpr does not ship a prebuilt binary for ${platform}-${arch}.`)
  }

  // Use whichever platform package the package manager installed: it already
  // filtered by `os`/`cpu`/`libc`, more reliable than re-deriving the host.
  let nativeBinary
  for (const target of candidates) {
    try {
      nativeBinary = require.resolve(target)
      break
    } catch {
      // Not installed for this host; try the next candidate.
    }
  }
  if (nativeBinary == null) {
    const pkgName = candidates[0].split('/').slice(0, 2).join('/')
    fail(
      `The "${pkgName}" package is not installed, so pnpr has no native binary to run.\n` +
      'If your package manager skipped optional dependencies or blocked build scripts, ' +
      'enable them and reinstall.'
    )
  }

  const binDir = path.join(ownDir, 'bin')
  if (platform === 'win32') {
    // The existing shim points at `bin/pnpr`, so that file must become the
    // binary; the `.exe` twin and `bin` rewrite are for shims generated later.
    placeBinary(nativeBinary, path.join(binDir, 'pnpr.exe'))
    placeBinary(nativeBinary, path.join(binDir, 'pnpr'))
    rewriteBin('bin/pnpr.exe')
  } else {
    placeBinary(nativeBinary, path.join(binDir, 'pnpr'), 0o755)
  }
}

/**
 * Atomically place `nativeBinary` at `destPath` (hard link, falling back to a
 * copy across filesystems, via a temp file + rename). Exits the process on
 * failure — without the binary there is no working `pnpr`.
 *
 * @param {string} nativeBinary Absolute path to the resolved native binary.
 * @param {string} destPath Absolute path to create.
 * @param {number} [mode] chmod for the copy path only; a hard link shares the
 *   source inode (the shared store blob under pnpm), so its mode must not change.
 */
function placeBinary (nativeBinary, destPath, mode) {
  const tempPath = `${destPath}.pnpr-tmp`
  try {
    fs.rmSync(tempPath, { force: true })
    let linked = false
    try {
      fs.linkSync(nativeBinary, tempPath)
      linked = true
    } catch {
      fs.copyFileSync(nativeBinary, tempPath)
    }
    if (!linked && mode != null) {
      fs.chmodSync(tempPath, mode)
    }
    fs.renameSync(tempPath, destPath)
  } catch (err) {
    try {
      fs.rmSync(tempPath, { force: true })
    } catch {
      // Nothing to clean up.
    }
    fail(`Could not install the pnpr binary at ${destPath}: ${err.message}`)
  }
}

function rewriteBin (binValue) {
  const pkgJsonPath = path.join(ownDir, 'package.json')
  // Write a fresh file and rename it over package.json rather than truncating in
  // place: pnpm hard-links package.json from its content-addressable store, so an
  // in-place write would mutate the shared store blob. Best-effort — it only
  // helps shims generated later.
  const tempPath = `${pkgJsonPath}.pnpr-tmp`
  try {
    const pkg = JSON.parse(fs.readFileSync(pkgJsonPath, 'utf8'))
    pkg.bin = binValue
    fs.writeFileSync(tempPath, JSON.stringify(pkg, null, 2))
    fs.renameSync(tempPath, pkgJsonPath)
  } catch {
    try {
      fs.rmSync(tempPath, { force: true })
    } catch {
      // Nothing to clean up.
    }
  }
}

function fail (message) {
  console.error(message)
  process.exit(1)
}

function readOwnManifest () {
  try {
    return JSON.parse(fs.readFileSync(path.join(ownDir, 'package.json'), 'utf8'))
  } catch {
    return {}
  }
}

/**
 * Native binary specifiers to try, most-preferred first; empty when the host is
 * unsupported. The linux glibc/musl pair is ordered by detected libc, which
 * only decides the winner when both are installed (e.g. `npm install --force`).
 *
 * @returns {string[]}
 */
function getBinCandidates () {
  const platformEntry = PLATFORMS?.[platform]?.[arch]

  if (platformEntry == null) {
    return []
  }
  if (typeof platformEntry === 'string') {
    return [platformEntry]
  }

  const order = detectLinuxLibc() === 'musl' ? ['musl', 'glibc'] : ['glibc', 'musl']
  return order.map((libc) => platformEntry[libc])
}

function detectLinuxLibc () {
  if (platform !== 'linux') {
    return null
  }

  // glibc builds set `glibcVersionRuntime`; musl leaves it unset. Guarded —
  // `process.report` may be unavailable, leaving ordering to the default.
  try {
    return process.report?.getReport().header.glibcVersionRuntime ? 'glibc' : 'musl'
  } catch {
    return null
  }
}
