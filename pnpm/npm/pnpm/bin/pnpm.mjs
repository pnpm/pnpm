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
// payload. What is left here is what it does not know: where to download from,
// what credentials to use, and whose signature to trust.
import { Buffer } from 'node:buffer'
import { spawnSync } from 'node:child_process'
import console from 'node:console'
import fs from 'node:fs'
import net from 'node:net'
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
  const { downloadPnpmExecutable } = await import(GET_PNPM).catch((err) => {
    fail(`This copy of the pnpm package is missing the downloader it needs: ${err.message}`)
  })
  const signature = signaturePolicy()
  try {
    await downloadPnpmExecutable({
      version,
      destPath: DOWNLOADED_BINARY,
      ...registryAccess({ readProjectNpmrc: signature.verifySignature !== false }),
      ...signature,
    })
  } catch (err) {
    fail(`Could not download the pnpm ${version} binary: ${err.message}`)
  }
  return DOWNLOADED_BINARY
}

/**
 * The registry to download from, and the headers to send it. Corepack's
 * `COREPACK_NPM_REGISTRY` wins. Otherwise npm's sources decide, highest first:
 * the environment, the project `.npmrc`, the user `.npmrc`.
 *
 * The project `.npmrc` is read only with `readProjectNpmrc`, which the caller
 * passes while the download is checked against npm's signature. The signature
 * pins the bytes, so a repository picks only where they come from. Unchecked,
 * it would pick the binary itself, which is kept next to this wrapper and run
 * in every later project (GHSA-j2hc-m6cf-6jm8). `get-pnpm` keeps the headers on
 * the registry's origin, so a download host it names never receives them.
 */
function registryAccess ({ readProjectNpmrc }) {
  const corepackRegistry = process.env.COREPACK_NPM_REGISTRY
  if (corepackRegistry) {
    return { registry: corepackRegistry, headers: corepackHeaders() }
  }
  const npmrc = readUserNpmrc()
  if (readProjectNpmrc) {
    Object.assign(npmrc, readNpmrc(path.join(projectDir(), '.npmrc'), projectEntry))
  }
  const registry = npmrc['@pnpm:registry'] || settingFromEnv('registry') || npmrc.registry
  if (!registry) {
    return { registry: DEFAULT_REGISTRY, headers: corepackHeaders() }
  }
  return { registry, headers: npmrcHeaders(npmrc, registry) }
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

function settingFromEnv (key) {
  const upper = key.toUpperCase()
  const { env } = process
  return env[`pnpm_config_${key}`] || env[`PNPM_CONFIG_${upper}`] ||
    env[`npm_config_${key}`] || env[`NPM_CONFIG_${upper}`] || undefined
}

/** The user `.npmrc`, with `${VAR}` placeholders expanded. */
function readUserNpmrc () {
  return readNpmrc(
    settingFromEnv('userconfig') || path.join(os.homedir(), '.npmrc'),
    (key, value) => [expandEnv(key), expandEnv(value)]
  )
}

/**
 * A project `.npmrc` entry, which is not expanded. One with a `${VAR}`
 * placeholder in a registry setting or a credential is dropped, as pnpm drops
 * it: a repository must not route the user's environment to a registry.
 */
function projectEntry (key, value, file) {
  if (!key.includes('${') && !value.includes('${')) {
    return [key, value]
  }
  if (key.startsWith('//') || key === 'registry' || key.endsWith(':registry')) {
    console.error(`Ignored ${key} from ${file}: a project .npmrc cannot use environment variables in registry settings or credentials.`)
  }
  return undefined
}

/** Where pnpm reads the project `.npmrc`: the workspace root, or else the current directory. */
function projectDir () {
  const cwd = process.cwd()
  for (let dir = cwd; ; dir = path.dirname(dir)) {
    if (fs.existsSync(path.join(dir, 'pnpm-workspace.yaml'))) {
      return dir
    }
    if (path.dirname(dir) === dir) {
      return cwd
    }
  }
}

/**
 * The top-level settings of the `.npmrc` at `file`, each passed through
 * `parseEntry`, which returns the `[key, value]` to keep or `undefined`.
 */
function readNpmrc (file, parseEntry) {
  const settings = Object.create(null)
  let text
  try {
    text = fs.readFileSync(file, 'utf8')
  } catch (err) {
    if (err.code !== 'ENOENT' && err.code !== 'EISDIR') {
      console.error(`Could not read ${file}, so its settings are not used: ${err.message}`)
    }
    return settings
  }
  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.trim()
    // Settings under a `[section]` are not top-level ones.
    if (line.startsWith('[')) {
      break
    }
    const separator = line.indexOf('=')
    if (separator === -1 || line.startsWith(';') || line.startsWith('#')) {
      continue
    }
    const entry = parseEntry(line.slice(0, separator).trim(), unquote(line.slice(separator + 1).trim()), file)
    if (entry != null) {
      settings[entry[0]] = entry[1]
    }
  }
  return settings
}

function unquote (value) {
  const quoted = value.length >= 2 && (value[0] === '"' || value[0] === "'") && value.at(-1) === value[0]
  return quoted ? value.slice(1, -1) : value
}

function expandEnv (value) {
  return value.replace(/\$\{([^${}]+)\}/g, (_, name) => process.env[name] ?? '')
}

/**
 * The credentials the `.npmrc` holds for `registry`, looked up as pnpm looks
 * them up for an `@pnpm` package: the `@pnpm` scope's own credentials first,
 * then the registry-wide ones. Each is the longest `//host[:port]/path/`
 * prefix of the registry URL that has any, then the same without the port.
 * They are sent only over HTTPS or to the local machine.
 */
function npmrcHeaders (npmrc, registry) {
  const url = new URL(registry.endsWith('/') ? registry : `${registry}/`)
  if (url.protocol !== 'https:' && !(url.protocol === 'http:' && isLoopback(url.hostname))) {
    return undefined
  }
  const prefixes = []
  for (const host of new Set([url.host, url.hostname])) {
    for (let pathname = url.pathname; ; pathname = pathname.slice(0, pathname.lastIndexOf('/', pathname.length - 2) + 1)) {
      prefixes.push(`//${host}${pathname}`)
      if (pathname === '/') {
        break
      }
    }
  }
  for (const scope of [':@pnpm', '']) {
    for (const prefix of prefixes) {
      const authorization = npmrcAuthorization(npmrc, `${prefix}${scope}`)
      if (authorization != null) {
        return { authorization }
      }
    }
  }
  return undefined
}

function isLoopback (hostname) {
  return hostname === 'localhost' || hostname === '[::1]' ||
    (net.isIPv4(hostname) && hostname.startsWith('127.'))
}

function npmrcAuthorization (npmrc, prefix) {
  const token = npmrc[`${prefix}:_authToken`]
  if (token) {
    return `Bearer ${token}`
  }
  const auth = npmrc[`${prefix}:_auth`]
  if (auth) {
    return `Basic ${auth}`
  }
  const username = npmrc[`${prefix}:username`]
  const password = npmrc[`${prefix}:_password`]
  if (username && password) {
    return `Basic ${base64(`${username}:${Buffer.from(password, 'base64').toString('utf8')}`)}`
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
