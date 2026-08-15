#!/usr/bin/env node
// Corepack's entry point into pnpm. Corepack hardcodes `./bin/pnpm.mjs` and
// `./bin/pnpx.mjs` for every pnpm >=11 (see its `config.json`) and loads them
// into its own Node process, which a native executable cannot be loaded into.
//
// Nothing else runs this file: `package.json#bin` still points at the native
// binary, so an ordinary `npm install -g pnpm` never pays for a Node startup.
//
// Corepack installs no dependencies and runs no lifecycle scripts, so the
// `@pnpm/exe.<target>` package that carries the binary is absent and
// `install.js` never ran. The binary is therefore downloaded on first use and
// kept next to this wrapper — where the native binary also finds the `dist/`
// payload it ships node-gyp in.
import { spawnSync } from 'node:child_process'
import console from 'node:console'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import {
  getBinCandidates,
  readWrapperManifest,
  resolveInstalledBinary,
  splitBinSpecifier,
  wrapperDir,
} from '../native-binary.mjs'
import { downloadNativeBinary } from './download-native-binary.mjs'

// Deliberately not the `pnpm` placeholder that `install.js` overwrites: a name
// of its own is what tells a downloaded binary apart from the placeholder.
const DOWNLOADED_BINARY = path.join(
  wrapperDir,
  process.platform === 'win32' ? 'pnpm-native.exe' : 'pnpm-native'
)

run(await nativeBinary())

function run (binary) {
  // Ctrl-C reaches the whole foreground process group, so the binary gets its
  // own SIGINT; exiting here first would hand the terminal back while it is
  // still shutting down.
  process.on('SIGINT', () => {})

  const result = spawnSync(binary, process.argv.slice(2), { stdio: 'inherit' })
  if (result.error != null) {
    fail(`Could not run the pnpm binary at ${binary}: ${result.error.message}`)
  }
  process.exitCode = result.signal == null
    ? result.status ?? 1
    : 128 + (os.constants.signals[result.signal] ?? 0)
}

async function nativeBinary () {
  const installed = resolveInstalledBinary()
  if (installed != null) {
    return installed
  }
  // A plain file, not merely something at that path: what a previous run left
  // is a file, and a directory there would be spawned as if it were a binary.
  if (fs.lstatSync(DOWNLOADED_BINARY, { throwIfNoEntry: false })?.isFile() === true) {
    return DOWNLOADED_BINARY
  }

  const candidates = getBinCandidates()
  if (candidates.length === 0) {
    fail(`pnpm does not ship a prebuilt binary for ${process.platform}-${process.arch}.`)
  }
  const { packageName, binFile } = splitBinSpecifier(candidates[0])

  // The version is pinned exactly, by the same release that published this
  // wrapper; the dev checkout has no `optionalDependencies` and no binary to
  // point them at.
  const version = readWrapperManifest().optionalDependencies?.[packageName]
  if (version == null) {
    fail(`This copy of the pnpm package declares no "${packageName}" dependency to take the binary from.`)
  }

  console.error(`Downloading the pnpm ${version} binary for ${process.platform}-${process.arch}...`)
  try {
    await downloadNativeBinary({ packageName, version, binFile, destPath: DOWNLOADED_BINARY })
  } catch (err) {
    fail(`Could not download ${packageName}@${version}: ${err.message}`)
  }
  return DOWNLOADED_BINARY
}

function fail (message) {
  console.error(message)
  process.exit(1)
}
