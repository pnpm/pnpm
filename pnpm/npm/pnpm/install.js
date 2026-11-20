#!/usr/bin/env node
// Native installs replace every bin with the host binary, so `pnpm` runs with
// no Node.js startup per call. The bins it replaces are shebang-less for the
// reason ./pnpm gives. npm's Windows shims still target the extensionless path
// after the `bin` rewrite, so postinstall asks npm to regenerate them against
// `pnpm.exe`. Installs that block lifecycle scripts keep the placeholders,
// which reach pnpm through Node.js wherever a shell runs them. Corepack runs no
// lifecycle scripts and enters through `bin/pnpm.mjs`.
import console from 'node:console'
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import {
  getBinCandidates,
  hostTarget,
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

  // The placeholders stay in place there and point to `@pnpm/wasm`.
  if ('webcontainer' in process.versions) {
    return
  }

  const candidates = getBinCandidates()
  if (candidates.length === 0) {
    fail(`pnpm does not ship a prebuilt binary for ${hostTarget()}.`)
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
    for (const name of BIN_NAMES) {
      placeBinary(nativeBinary, path.join(wrapperDir, name), 0o755)
    }
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
    removeFileIfPossible(tempPath)
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
    npmExecPath == null ||
    path.basename(npmExecPath).toLowerCase() !== 'npm-cli.js'
  ) {
    return
  }

  const packageName = readWrapperManifest().name
  if (typeof packageName !== 'string') {
    fail('Could not determine the pnpm wrapper package name when regenerating npm shims.')
  }
  const args = [
    npmExecPath,
    'rebuild',
    '--ignore-scripts',
  ]
  if (process.env.npm_config_global === 'true' || process.env.npm_config_location === 'global') {
    args.push('--global', packageName)
  } else {
    const prefix = findNpmProjectPrefix()
    if (prefix == null) {
      return
    }
    args.push('--prefix', prefix, packageName)
  }
  const result = spawnSync(process.execPath, args, { stdio: 'inherit' })
  if (result.error != null) {
    fail(`Could not regenerate the npm shims for pnpm: ${result.error.message}`)
  }
  if (result.status !== 0) {
    fail('npm could not regenerate the shims for pnpm.')
  }
}

/**
 * The resolved path of the npm project whose `node_modules` holds this
 * wrapper. Returns `null` when npm names no project, when its `node_modules`
 * cannot be resolved, or when it does not contain the wrapper. `npm exec`
 * installs into its own cache while the prefix still names the caller's
 * project, and rebuilding there would touch an unrelated project.
 *
 * @returns {string | null}
 */
function findNpmProjectPrefix () {
  const prefix = process.env.npm_config_local_prefix
  if (typeof prefix !== 'string' || prefix === '') {
    return null
  }
  let realPrefix
  let realModulesDir
  try {
    realPrefix = fs.realpathSync(prefix)
    realModulesDir = fs.realpathSync(path.join(realPrefix, 'node_modules'))
  } catch {
    return null
  }
  // `wrapperDir` comes from the module URL, which Node resolves through symlinks.
  const relative = path.relative(realModulesDir, wrapperDir)
  if (relative === '' || relative.split(path.sep)[0] === '..' || path.isAbsolute(relative)) {
    return null
  }
  return realPrefix
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
