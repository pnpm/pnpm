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
// The download itself is `get-pnpm`, the package behind https://get.pnpm.io,
// which already knows how to verify one; it travels in that same `dist/`
// payload, and reads pnpm's registry config. What is left here is Corepack's
// own: its registry and credentials, and whose signature to trust.
import { Buffer } from 'node:buffer'
import { spawnSync } from 'node:child_process'
import console from 'node:console'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { URL } from 'node:url'
import { readWrapperManifest, resolveInstalledBinary, wrapperDir } from '../native-binary.mjs'

// Deliberately not the `pnpm` placeholder that `install.js` overwrites: a name
// of its own is what tells a downloaded binary apart from the placeholder.
const DOWNLOADED_BINARY = path.join(
  wrapperDir,
  process.platform === 'win32' ? 'pnpm-native.exe' : 'pnpm-native'
)
const GET_PNPM = new URL('../dist/node_modules/get-pnpm/lib/index.js', import.meta.url)
const DEFAULT_REGISTRY = 'https://registry.npmjs.org'

if ('webcontainer' in process.versions) {
  fail('The pnpm package runs a native binary, which WebContainers cannot execute. Install @pnpm/wasm instead.')
}
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

  if (process.env.COREPACK_ENABLE_NETWORK === '0') {
    fail('Network access is disabled by the environment, so the pnpm binary cannot be downloaded.')
  }

  const { version } = readWrapperManifest()
  console.error(`Downloading the pnpm ${version} binary for ${process.platform}-${process.arch}...`)
  const { downloadPnpmExecutable, pnpmRegistryConfig } = await import(GET_PNPM).catch((err) => {
    fail(`This copy of the pnpm package is missing the downloader it needs: ${err.message}`)
  })
  const signature = signaturePolicy()
  try {
    await downloadPnpmExecutable({
      version,
      destPath: DOWNLOADED_BINARY,
      ...registryAccess(pnpmRegistryConfig, { readProjectNpmrc: signature.verifySignature !== false }),
      ...signature,
    })
  } catch (err) {
    fail(`Could not download the pnpm ${version} binary: ${err.message}`)
  }
  return DOWNLOADED_BINARY
}

/**
 * The registry to download from, and the credentials and CA certificates for
 * it. Corepack's `COREPACK_NPM_REGISTRY` wins. Otherwise `pnpmRegistryConfig`
 * reads pnpm's config, and Corepack's credentials go to the npm registry only
 * when that config holds none.
 *
 * The project `.npmrc` is read only with `readProjectNpmrc`, which the caller
 * passes while the download is checked against npm's signature
 * (GHSA-j2hc-m6cf-6jm8).
 */
function registryAccess (pnpmRegistryConfig, { readProjectNpmrc }) {
  const corepackRegistry = process.env.COREPACK_NPM_REGISTRY
  if (corepackRegistry) {
    return { registry: corepackRegistry, headers: corepackHeaders() }
  }
  const config = pnpmRegistryConfig({ readProjectNpmrc })
  if (config.headers == null && new URL(config.registry).origin === DEFAULT_REGISTRY) {
    return { ...config, headers: corepackHeaders() }
  }
  return config
}

/** Credentials for Corepack's registry, read the way Corepack reads its own. */
function corepackHeaders () {
  const { COREPACK_NPM_TOKEN, COREPACK_NPM_USERNAME, COREPACK_NPM_PASSWORD } = process.env
  if (COREPACK_NPM_TOKEN) {
    return { authorization: `Bearer ${COREPACK_NPM_TOKEN}` }
  }
  if (COREPACK_NPM_USERNAME && COREPACK_NPM_PASSWORD) {
    return { authorization: `Basic ${base64(`${COREPACK_NPM_USERNAME}:${COREPACK_NPM_PASSWORD}`)}` }
  }
  return undefined
}

function base64 (text) {
  return Buffer.from(text, 'utf8').toString('base64')
}

/**
 * Whose signature over the download to trust, following `COREPACK_INTEGRITY_KEYS`
 * exactly as Corepack does: npm's own keys when it is unset, the keys it names
 * when it holds a key set, and no signature check at all when it is `0` or
 * empty — which is the state a registry that re-publishes packages, and
 * therefore carries no npm signatures, already has to be in for Corepack to
 * have installed this wrapper from it.
 */
function signaturePolicy () {
  const configured = process.env.COREPACK_INTEGRITY_KEYS
  if (configured == null) {
    return {}
  }
  if (configured === '' || configured === '0') {
    return { verifySignature: false }
  }
  let keys
  try {
    keys = JSON.parse(configured).npm
  } catch (err) {
    fail(`COREPACK_INTEGRITY_KEYS is not readable as JSON: ${err.message}`)
  }
  // An absent or malformed `npm` entry would otherwise be passed on as no keys
  // at all, which falls back to npm's own — the opposite of what setting the
  // variable asked for. An empty set is left alone: it names no key to trust,
  // and Corepack reads it the same way, so every download is refused.
  if (!Array.isArray(keys)) {
    fail('COREPACK_INTEGRITY_KEYS holds no "npm" key set to verify the pnpm binary against.')
  }
  return { keys }
}

function fail (message) {
  console.error(message)
  process.exit(1)
}
