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
//
// Corepack runs no lifecycle scripts, so it never gets here; it enters through
// `bin/pnpm.mjs` instead.
import console from 'node:console'
import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import {
  getBinCandidates,
  readWrapperManifest,
  resolveInstalledBinary,
  splitBinSpecifier,
  wrapperDir,
} from './native-binary.mjs'

const BIN_NAMES = ['pnpm', 'pn', 'pnpx', 'pnx']

setup()

function setup () {
  // The committed manifest has no `optionalDependencies`; generate-packages.mjs
  // adds them at release time. Without them this is the monorepo checkout, where
  // the wrapper is a workspace package and there is no native binary to link.
  if (readWrapperManifest().optionalDependencies == null) {
    return
  }

  const candidates = getBinCandidates()
  if (candidates.length === 0) {
    fail(`pnpm does not ship a prebuilt binary for ${process.platform}-${process.arch}.`)
  }

  const nativeBinary = resolveInstalledBinary()
  if (nativeBinary == null) {
    const { packageName } = splitBinSpecifier(candidates[0])
    fail(
      `The "${packageName}" package is not installed, so pnpm has no native binary to run.\n` +
      'If your package manager skipped optional dependencies or blocked build scripts, ' +
      'enable them and reinstall.'
    )
  }

  if (process.platform === 'win32') {
    const newBin = {}
    for (const name of BIN_NAMES) {
      // The existing shim points at the no-ext file, so it must become the
      // binary; the `.exe` twin + bin rewrite are for shims generated later.
      placeBinary(nativeBinary, path.join(wrapperDir, `${name}.exe`))
      placeBinary(nativeBinary, path.join(wrapperDir, name))
      newBin[name] = `${name}.exe`
    }
    rewriteBin(newBin)
  } else {
    placeBinary(nativeBinary, path.join(wrapperDir, 'pnpm'), 0o755)
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
  const pkgJsonPath = path.join(wrapperDir, 'package.json')
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
