#!/usr/bin/env node
// Preinstall for the pnpm v12 wrapper (shared verbatim by `pnpm` and
// `@pnpm/exe`): replace the shebang-less placeholder bins with the host's native
// binary so `pnpm` runs directly, no Node startup per call. A placeholder (not a
// Node launcher) is required because the Windows shim is generated from the bin
// file and npm won't re-read package.json after preinstall; the tradeoff is no
// fallback when build scripts are blocked (`--ignore-scripts`, pnpm/Bun default).
//
// `pn`/`pnpx`/`pnx` are committed `#!/bin/sh` scripts on Unix (so only `pnpm` is
// relinked); on Windows the native binary is hardlinked onto each and
// self-detects its launch name to inject `dlx` (see `argv_with_alias_subcommand`
// in the cli crate).
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
    x64: '@pnpm/exe.win32-x64/pnpm.exe',
    arm64: '@pnpm/exe.win32-arm64/pnpm.exe',
  },
  darwin: {
    x64: '@pnpm/exe.darwin-x64/pnpm',
    arm64: '@pnpm/exe.darwin-arm64/pnpm',
  },
  linux: {
    x64: {
      glibc: '@pnpm/exe.linux-x64/pnpm',
      musl: '@pnpm/exe.linux-x64-musl/pnpm',
    },
    arm64: {
      glibc: '@pnpm/exe.linux-arm64/pnpm',
      musl: '@pnpm/exe.linux-arm64-musl/pnpm',
    },
  },
}

const BIN_NAMES = ['pnpm', 'pn', 'pnpx', 'pnx']

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
    fail(`pnpm does not ship a prebuilt binary for ${platform}-${arch}.`)
  }

  // Use whichever platform package the package manager installed: it already
  // filtered by `os`/`cpu`/`libc`, more reliable than re-deriving the host.
  let nativeBinary
  for (const target of candidates) {
    try {
      nativeBinary = require.resolve(target)
      break
    } catch {}
  }
  if (nativeBinary == null) {
    const pkgName = candidates[0].split('/').slice(0, 2).join('/')
    fail(
      `The "${pkgName}" package is not installed, so pnpm has no native binary to run.\n` +
      'If your package manager skipped optional dependencies or blocked build scripts, ' +
      'enable them and reinstall.'
    )
  }

  if (platform === 'win32') {
    const newBin = {}
    for (const name of BIN_NAMES) {
      // The existing shim points at the no-ext file, so it must become the
      // binary; the `.exe` twin + bin rewrite are for shims generated later.
      placeBinary(nativeBinary, path.join(ownDir, `${name}.exe`))
      placeBinary(nativeBinary, path.join(ownDir, name))
      newBin[name] = `${name}.exe`
    }
    rewriteBin(newBin)
  } else {
    placeBinary(nativeBinary, path.join(ownDir, 'pnpm'), 0o755)
  }
}

/**
 * Atomically place `nativeBinary` at `destPath` (hard link, falling back to a
 * copy across filesystems, via a temp file + rename). Exits the process on
 * failure — without the binary there is no working `pnpm`.
 *
 * @param {string} nativeBinary Absolute path to the resolved native binary.
 * @param {string} destPath Absolute path to create.
 * @param {number} [mode] chmod for the copy path only; a hard link shares the
 *   source inode (the shared store blob under pnpm), so its mode must not change.
 */
function placeBinary (nativeBinary, destPath, mode) {
  const tempPath = `${destPath}.pnpm-tmp`
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
    } catch {}
    fail(`Could not install the pnpm binary at ${destPath}: ${err.message}`)
  }
}

function rewriteBin (binMap) {
  const pkgJsonPath = path.join(ownDir, 'package.json')
  // Temp file + rename, not in-place: package.json is hard-linked from the store.
  const tempPath = `${pkgJsonPath}.pnpm-tmp`
  try {
    const pkg = JSON.parse(fs.readFileSync(pkgJsonPath, 'utf8'))
    pkg.bin = binMap
    fs.writeFileSync(tempPath, JSON.stringify(pkg, null, 2))
    fs.renameSync(tempPath, pkgJsonPath)
  } catch {
    try {
      fs.rmSync(tempPath, { force: true })
    } catch {}
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
