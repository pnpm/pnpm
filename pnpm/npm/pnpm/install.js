#!/usr/bin/env node
// Preinstall for the pnpm v12 wrapper (shared verbatim by `pnpm` and
// `@pnpm/exe`): replace the placeholder bins with the host's native binary so
// `pnpm` runs directly, no Node startup per call. npm generates the Windows
// shim before preinstall and won't re-read package.json during that install
// pass, which is what shapes the placeholder (see the `pnpm` file); on a global
// Windows install, postinstall asks npm to relink after the bin rewrite. When
// build scripts are blocked (`--ignore-scripts`, the pnpm/Bun default) the
// placeholder stays: on Unix it runs as a `sh` script that hands over to
// `bin/pnpm.mjs`, on Windows it does not run.
//
// `pn`/`pnpx`/`pnx` are committed `#!/bin/sh` scripts on Unix (so only `pnpm` is
// relinked); on Windows the native binary is hardlinked onto each and
// self-detects its launch name to inject `dlx` (see `argv_with_alias_subcommand`
// in the cli crate).
//
// Corepack runs no lifecycle scripts, so it never gets here; it enters through
// `bin/pnpm.mjs` too.
import console from 'node:console'
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import {
  getBinCandidates,
  placeBinary,
  readWrapperManifest,
  resolveInstalledBinary,
  splitBinSpecifier,
  wrapperDir,
} from './native-binary.mjs'

const BIN_NAMES = ['pnpm', 'pn', 'pnpx', 'pnx']

if (process.env.npm_lifecycle_event === 'postinstall') {
  relinkNpmWindowsShims()
} else {
  setup()
}

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
      placeBinaryOrExit(nativeBinary, path.join(wrapperDir, `${name}.exe`))
      placeBinaryOrExit(nativeBinary, path.join(wrapperDir, name))
      newBin[name] = `${name}.exe`
    }
    rewriteBin(newBin)
  } else {
    placeBinaryOrExit(nativeBinary, path.join(wrapperDir, 'pnpm'), 0o755)
  }
}

/**
 * {@link placeBinary}, exiting the process on failure — without the binary
 * there is no working `pnpm`.
 */
function placeBinaryOrExit (nativeBinary, destPath, mode) {
  try {
    placeBinary(nativeBinary, destPath, mode)
  } catch (err) {
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
    removeFileIfPossible(tempPath)
  }
}

function relinkNpmWindowsShims () {
  const npmExecPath = process.env.npm_execpath
  if (
    process.platform !== 'win32' ||
    process.env.npm_config_global !== 'true' ||
    npmExecPath == null ||
    path.basename(npmExecPath).toLowerCase() !== 'npm-cli.js'
  ) {
    return
  }

  const packageName = readWrapperManifest().name
  if (typeof packageName !== 'string') {
    fail('Could not determine the pnpm wrapper package name when regenerating npm shims.')
  }
  const result = spawnSync(process.execPath, [
    npmExecPath,
    'rebuild',
    '--global',
    '--ignore-scripts',
    packageName,
  ], { stdio: 'inherit' })
  if (result.error != null) {
    fail(`Could not regenerate the npm shims for pnpm: ${result.error.message}`)
  }
  if (result.status !== 0) {
    fail('npm could not regenerate the shims for pnpm.')
  }
}

function removeFileIfPossible (filePath) {
  try {
    fs.rmSync(filePath, { force: true })
  } catch {
    return
  }
}

function fail (message) {
  console.error(message)
  process.exit(1)
}
