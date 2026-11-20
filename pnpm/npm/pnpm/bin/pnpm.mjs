#!/usr/bin/env node
// Corepack's entry point into pnpm. Corepack hardcodes `./bin/pnpm.mjs` and
// `./bin/pnpx.mjs` for every pnpm >=11 (see its `config.json`) and loads them
// into its own Node.js process, which a native executable cannot be loaded into.
//
// The `pnpm` placeholder bin runs it too, on Unix, when the install script that
// replaces it with the native binary did not (build scripts blocked). An
// ordinary `npm install -g pnpm` never pays for a Node.js startup:
// `package.json#bin` points at the native binary.
//
// Corepack installs no dependencies and runs no lifecycle scripts, so the
// `@pnpm/exe.<target>` package that carries the binary is absent and
// `install.js` never ran. The binary is therefore downloaded on first use and
// kept next to this wrapper — where the native binary also finds the `dist/`
// payload it ships node-gyp in. The placeholder's installs usually do carry
// that package, and the binary is taken from there.
//
// The download itself is `get-pnpm`, the installer in ../../get-pnpm,
// which already knows how to verify one; it travels in that same `dist/`
// payload, and reads the registry, credentials, and signing keys from pnpm's
// and Corepack's config.
import { spawn, spawnSync } from 'node:child_process'
import console from 'node:console'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { setImmediate } from 'node:timers/promises'
import { URL } from 'node:url'
import { readWrapperManifest, resolveInstalledBinary, wrapperDir } from '../native-binary.mjs'

// Deliberately not the `pnpm` placeholder that `install.js` overwrites: a name
// of its own is what tells a downloaded binary apart from the placeholder.
const DOWNLOADED_BINARY = path.join(
  wrapperDir,
  process.platform === 'win32' ? 'pnpm-native.exe' : 'pnpm-native'
)
const GET_PNPM = new URL('../dist/node_modules/get-pnpm/lib/index.js', import.meta.url)

if ('webcontainer' in process.versions) {
  fail('The pnpm package runs a native binary, which WebContainers cannot execute. Install @pnpm/wasm instead.')
}
await run(await nativeBinary())

async function run (binary) {
  // Ctrl-C reaches the whole foreground process group, so the binary gets its
  // own SIGINT; exiting here first would hand the terminal back while it is
  // still shutting down.
  let interrupted = false
  const onInterrupt = () => { interrupted = true }
  process.on('SIGINT', onInterrupt)
  const args = process.argv.slice(2)
  const env = childEnv(binary)
  let result = process.platform === 'win32'
    ? spawnSync(binary, args, { stdio: 'inherit', env })
    : await runUnix(binary, args, env, () => interrupted)
  process.removeListener('SIGINT', onInterrupt)
  if (result.shellMissing === true) {
    // A shell-free image can still run pnpm. Leave Node's default SIGINT
    // behavior in place until the foreground child exists.
    if (interrupted) {
      process.exitCode = 130
      return
    }
    const child = spawn(binary, args, { stdio: 'inherit', env })
    process.on('SIGINT', onInterrupt)
    result = await childResult(child)
    process.removeListener('SIGINT', onInterrupt)
  }
  if (result.error != null) {
    fail(`Could not run the pnpm binary at ${binary}: ${result.error.message}`)
  }
  process.exitCode = result.signal == null
    ? result.status ?? 1
    : 128 + (os.constants.signals[result.signal] ?? 0)
}

function childEnv (binary) {
  // Binaries beside the wrapper can discover the payload on their own.
  if (path.dirname(binary) === wrapperDir) return undefined
  const binDir = path.join(wrapperDir, 'dist', 'node-gyp-bin')
  const wrapper = path.join(binDir, process.platform === 'win32' ? 'node-gyp.cmd' : 'node-gyp')
  const entry = path.join(wrapperDir, 'dist', 'node_modules', 'node-gyp', 'bin', 'node-gyp.js')
  if (binDir.includes(path.delimiter) ||
      fs.statSync(wrapper, { throwIfNoEntry: false })?.isFile() !== true ||
      fs.statSync(entry, { throwIfNoEntry: false })?.isFile() !== true) return undefined

  const env = { ...process.env }
  const envKey = (name) => process.platform === 'win32'
    ? Object.keys(env).find(key => key.toLowerCase() === name.toLowerCase()) ?? name
    : name
  const pathKey = envKey('PATH')
  const nodeGypKey = envKey('npm_config_node_gyp')
  env[pathKey] = env[pathKey] ? `${binDir}${path.delimiter}${env[pathKey]}` : binDir
  env[nodeGypKey] ||= entry
  return env
}

async function runUnix (binary, args, env, wasInterrupted) {
  try {
    fs.accessSync(binary, fs.constants.X_OK)
  } catch (error) {
    return { error }
  }
  // Keep the child in the foreground group and hold it at a pipe until Node
  // has had a chance to dispatch a SIGINT received before the child existed.
  const shell = process.platform === 'android' ? '/system/bin/sh' : '/bin/sh'
  const child = spawn(shell, ['-c', 'read -r launch <&3 || exit 130\nexec 3<&-\nexec "$@"', 'pnpm', binary, ...args], {
    stdio: ['inherit', 'inherit', 'inherit', 'pipe'],
    env,
  })
  const launch = child.stdio[3]
  const childResultPromise = childResult(child)
  const pipeError = new Promise((resolve) => {
    launch?.once('error', (error) => {
      if (error.code !== 'EPIPE' && error.code !== 'ECONNRESET') resolve({ error })
    })
  })
  // A single immediate can run in the current check phase. Two leave a poll
  // phase in between for Node's queued signal callback.
  await setImmediate()
  await setImmediate()
  if (wasInterrupted()) {
    launch?.end()
  } else {
    launch?.end('run\n')
  }
  const result = await Promise.race([childResultPromise, pipeError])
  return result.error?.code === 'ENOENT' && child.pid == null ? { shellMissing: true } : result
}

function childResult (child) {
  return new Promise((resolve) => {
    child.once('error', (error) => resolve({ error }))
    child.once('close', (status, signal) => resolve({ status, signal }))
  })
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

  const { version } = readWrapperManifest()
  const { corepackDownloadOptions, downloadPnpmExecutable } = await import(GET_PNPM).catch((err) => {
    fail(`This copy of the pnpm package is missing the downloader it needs: ${err.message}`)
  })
  let options
  try {
    options = corepackDownloadOptions()
  } catch (err) {
    fail(err.message)
  }
  console.error(`Downloading the pnpm ${version} binary for ${process.platform}-${process.arch}...`)
  try {
    await downloadPnpmExecutable({ version, destPath: DOWNLOADED_BINARY, ...options })
  } catch (err) {
    fail(`Could not download the pnpm ${version} binary: ${err.message}`)
  }
  return DOWNLOADED_BINARY
}

function fail (message) {
  console.error(message)
  process.exit(1)
}
