// Exercises the Corepack entry points (`bin/pnpm.mjs`, `bin/pnpx.mjs`) the way
// Corepack uses them: a wrapper directory without the `@pnpm/exe.<target>`
// dependency, against a registry that serves a stand-in for the native binary.
//
// The stand-in is a shell script, so only the tests that reach the point of
// spawning it are Unix-only. Everything that ends before that — a refused
// download, a misconfigured key set, a forbidden network — runs everywhere,
// which is what covers the paths where Windows differs.
import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import fs from 'node:fs'
import { createRequire } from 'node:module'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { after, describe, it } from 'node:test'
import tls from 'node:tls'
import { URL, fileURLToPath } from 'node:url'

import { INTEGRITY_KEYS, TLS_CA_FILE, VERSION, binFile, packageName, startRegistry } from './registry-fixture.mjs'

const WRAPPER_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const WRAPPER_FILES = ['native-binary.mjs', 'bin/pnpm.mjs', 'bin/pnpx.mjs']
// Where the published wrapper carries get-pnpm and its dependencies;
// scripts/bundle-node-gyp.mjs puts them there, and the fixture stands in for that.
const BUNDLED_MODULES = 'dist/node_modules'
// A stand-in for the 40 MB native binary: reports how it was called, and turns
// a `fail` argument into a non-zero exit so exit codes can be asserted.
const FAKE_BINARY = `#!/bin/sh
if [ "$1" = fail ]; then exit 3; fi
echo "ran: $*"
`

const SPAWNS_THE_BINARY = process.platform === 'win32' &&
  'the stand-in for the native binary is a shell script, which Windows cannot spawn'
const DOWNLOADED_BINARY = process.platform === 'win32' ? 'pnpm-native.exe' : 'pnpm-native'
const ADDS_DEFAULT_CAS = tls.setDefaultCACertificates == null &&
  'this Node.js cannot add a CA to the ones fetch trusts after startup'
// The variables that could name a registry or a user `.npmrc` from the
// environment the tests run in.
const ENV_NPM_CONFIG = ['registry', 'userconfig'].flatMap((name) => [
  `pnpm_config_${name}`,
  `PNPM_CONFIG_${name.toUpperCase()}`,
  `npm_config_${name}`,
  `NPM_CONFIG_${name.toUpperCase()}`,
])

describe('corepack entry point', () => {
  it('downloads the native binary on first use, then reuses it', { skip: SPAWNS_THE_BINARY }, async () => {
    const fixture = await createFixture()

    const first = await runEntry(fixture, 'bin/pnpm.mjs', ['--version'])
    assert.equal(first.status, 0, first.stderr)
    assert.match(first.stdout, /ran: --version/)
    assert.match(first.stderr, /Downloading the pnpm 99\.0\.0 binary/)
    assert.ok(fs.existsSync(path.join(fixture.dir, DOWNLOADED_BINARY)))

    // Nothing but the cached binary can answer once the registry is gone.
    await fixture.closeRegistry()
    const second = await runEntry(fixture, 'bin/pnpm.mjs', ['install'])
    assert.equal(second.status, 0, second.stderr)
    assert.match(second.stdout, /ran: install/)
    assert.doesNotMatch(second.stderr, /Downloading/)
  })

  it('propagates the exit code of the native binary', { skip: SPAWNS_THE_BINARY }, async () => {
    const fixture = await createFixture()

    assert.equal((await runEntry(fixture, 'bin/pnpm.mjs', ['fail'])).status, 3)
  })

  it('runs `pnpx` as `pnpm dlx`', { skip: SPAWNS_THE_BINARY }, async () => {
    const fixture = await createFixture()

    const result = await runEntry(fixture, 'bin/pnpx.mjs', ['create-vite', 'app'])
    assert.equal(result.status, 0, result.stderr)
    assert.match(result.stdout, /ran: dlx create-vite app/)
  })

  it('prefers the installed platform package over a download', { skip: SPAWNS_THE_BINARY }, async () => {
    const fixture = await createFixture()
    const installedDir = path.join(fixture.dir, 'node_modules', packageName)
    fs.mkdirSync(installedDir, { recursive: true })
    fs.writeFileSync(path.join(installedDir, 'package.json'), JSON.stringify({ name: packageName, version: VERSION }))
    fs.writeFileSync(path.join(installedDir, binFile), FAKE_BINARY.replace('ran:', 'installed:'), { mode: 0o755 })

    const result = await runEntry(fixture, 'bin/pnpm.mjs', ['--version'])
    assert.equal(result.status, 0, result.stderr)
    assert.match(result.stdout, /installed: --version/)
    assert.equal(fs.existsSync(path.join(fixture.dir, DOWNLOADED_BINARY)), false)
  })

  it('refuses a download the published checksum does not cover', async () => {
    const fixture = await createFixture({ tamper: true })

    const result = await runEntry(fixture, 'bin/pnpm.mjs', ['--version'])
    assert.notEqual(result.status, 0)
    assert.match(result.stderr, /does not match the checksum/)
    assert.equal(fs.existsSync(path.join(fixture.dir, DOWNLOADED_BINARY)), false)
  })

  it('refuses a download signed by a key the environment does not name', async () => {
    const fixture = await createFixture()

    const result = await runEntry(fixture, 'bin/pnpm.mjs', ['--version'], {
      COREPACK_INTEGRITY_KEYS: JSON.stringify({ npm: [] }),
    })
    assert.notEqual(result.status, 0)
    assert.match(result.stderr, /unexpected npm key/)
  })

  it('refuses a registry that publishes no signature', async () => {
    const fixture = await createFixture({ unsigned: true })

    const result = await runEntry(fixture, 'bin/pnpm.mjs', ['--version'])
    assert.notEqual(result.status, 0)
    assert.match(result.stderr, /carries no npm registry signature/)
  })

  // Corepack's own opt-out, which a registry that publishes no signature
  // already needs for Corepack to have installed this wrapper from it.
  it('takes an unsigned download when Corepack is told to skip', { skip: SPAWNS_THE_BINARY }, async () => {
    const fixture = await createFixture({ unsigned: true })

    const result = await runEntry(fixture, 'bin/pnpm.mjs', ['--version'], { COREPACK_INTEGRITY_KEYS: '0' })
    assert.equal(result.status, 0, result.stderr)
    assert.match(result.stdout, /ran: --version/)
  })

  it('reports a key set it cannot read instead of silently trusting npm', async () => {
    const fixture = await createFixture()

    const unreadable = await runEntry(fixture, 'bin/pnpm.mjs', ['--version'], { COREPACK_INTEGRITY_KEYS: '{' })
    assert.notEqual(unreadable.status, 0)
    assert.match(unreadable.stderr, /not readable as JSON/)

    const empty = await runEntry(fixture, 'bin/pnpm.mjs', ['--version'], { COREPACK_INTEGRITY_KEYS: '{"other":[]}' })
    assert.notEqual(empty.status, 0)
    assert.match(empty.stderr, /no "npm" key set/)
  })

  it('downloads from the registry the user .npmrc names, with its credentials', async () => {
    const fixture = await createFixture()
    const userNpmrc = writeNpmrc(fixture, 'user.npmrc', [
      `registry=${fixture.registryUrl}/`,
      `${nerfDart(fixture.registryUrl)}:_authToken=\${FIXTURE_TOKEN}`,
    ])

    const result = await runEntry(fixture, 'bin/pnpm.mjs', ['--version'], {
      ...userNpmConfig(userNpmrc),
      FIXTURE_TOKEN: 'user-token',
      COREPACK_NPM_TOKEN: 'corepack-token',
    })
    assert.match(result.stderr, /Downloading/)
    assert.deepEqual(new Set(fixture.requests.map(({ url, authorization }) => `${url} ${authorization}`)), new Set([
      `/${packageName}/${VERSION} Bearer user-token`,
      `/${packageName}/-/${VERSION}.tgz Bearer user-token`,
    ]))
  })

  it('takes the @pnpm scope registry over the default one', async () => {
    const fixture = await createFixture()
    const userNpmrc = writeNpmrc(fixture, 'user.npmrc', [`@pnpm:registry=${fixture.registryUrl}/`])

    await runEntry(fixture, 'bin/pnpm.mjs', ['--version'], {
      ...userNpmConfig(userNpmrc),
      npm_config_registry: 'http://127.0.0.1:9/',
    })
    assert.ok(fixture.requests.some(({ url }) => url === `/${packageName}/${VERSION}`))
  })

  it('sends the @pnpm scope credentials over the registry-wide ones', async () => {
    const fixture = await createFixture()
    const userNpmrc = writeNpmrc(fixture, 'user.npmrc', [
      `@pnpm:registry=${fixture.registryUrl}/`,
      `${nerfDart(fixture.registryUrl)}:_authToken=registry-token`,
      `${nerfDart(fixture.registryUrl)}:@pnpm:_authToken=scope-token`,
    ])

    await runEntry(fixture, 'bin/pnpm.mjs', ['--version'], userNpmConfig(userNpmrc))
    assert.deepEqual(new Set(fixture.requests.map(({ url, authorization }) => `${url} ${authorization}`)), new Set([
      `/${packageName}/${VERSION} Bearer scope-token`,
      `/${packageName}/-/${VERSION}.tgz Bearer scope-token`,
    ]))
  })

  it('downloads from the registry the workspace .npmrc names, without expanding its credentials', async () => {
    const fixture = await createFixture()
    const userRegistry = await startRegistry({ payload: FAKE_BINARY })
    after(userRegistry.close)
    fs.writeFileSync(path.join(fixture.projectDir, 'pnpm-workspace.yaml'), '')
    writeNpmrc(fixture, 'project/.npmrc', [
      `registry=${fixture.registryUrl}/`,
      `${nerfDart(fixture.registryUrl)}:_authToken=\${FIXTURE_TOKEN}`,
    ])
    const userNpmrc = writeNpmrc(fixture, 'user.npmrc', [`registry=${userRegistry.url}/`])
    const packageDir = path.join(fixture.projectDir, 'packages/a')
    fs.mkdirSync(packageDir, { recursive: true })

    const result = await runEntry({ ...fixture, projectDir: packageDir }, 'bin/pnpm.mjs', ['--version'], {
      ...userNpmConfig(userNpmrc),
      FIXTURE_TOKEN: 'user-token',
    })
    assert.match(result.stderr, /Ignored .*:_authToken from .*\.npmrc/)
    assert.deepEqual(userRegistry.requests, [])
    assert.deepEqual(new Set(fixture.requests.map(({ url, authorization }) => `${url} ${authorization}`)), new Set([
      `/${packageName}/${VERSION} undefined`,
      `/${packageName}/-/${VERSION}.tgz undefined`,
    ]))
  })

  it('does not let a project .npmrc choose the registry of an unchecked download', async () => {
    const fixture = await createFixture()
    const projectRegistry = await startRegistry({ payload: FAKE_BINARY })
    after(projectRegistry.close)
    writeNpmrc(fixture, 'project/.npmrc', [`registry=${projectRegistry.url}/`])
    const userNpmrc = writeNpmrc(fixture, 'user.npmrc', [`registry=${fixture.registryUrl}/`])

    await runEntry(fixture, 'bin/pnpm.mjs', ['--version'], {
      ...userNpmConfig(userNpmrc),
      COREPACK_INTEGRITY_KEYS: '0',
    })
    assert.deepEqual(projectRegistry.requests, [])
    assert.ok(fixture.requests.some(({ url }) => url === `/${packageName}/${VERSION}`))
  })

  it('refuses a registry certificate that no configured CA vouches for', async () => {
    const fixture = await createFixture({ tls: true })
    const userNpmrc = writeNpmrc(fixture, 'user.npmrc', [`registry=${fixture.registryUrl}/`])

    const result = await runEntry(fixture, 'bin/pnpm.mjs', ['--version'], userNpmConfig(userNpmrc))
    assert.notEqual(result.status, 0)
    assert.match(result.stderr, /Could not reach/)
    assert.deepEqual(fixture.requests, [])
  })

  it('trusts the CA in the cafile the user .npmrc names', { skip: ADDS_DEFAULT_CAS }, async () => {
    const fixture = await createFixture({ tls: true })
    const userNpmrc = writeNpmrc(fixture, 'user.npmrc', [
      `registry=${fixture.registryUrl}/`,
      `cafile=${TLS_CA_FILE}`,
    ])

    const result = await runEntry(fixture, 'bin/pnpm.mjs', ['--version'], userNpmConfig(userNpmrc))
    assertDownloaded(fixture, result)
  })

  it('trusts every inline ca[] entry of the user .npmrc', { skip: ADDS_DEFAULT_CAS }, async () => {
    const fixture = await createFixture({ tls: true })
    // npm writes a PEM on one line, its line breaks escaped. The fixture CA
    // comes first, so keeping only the last entry would not trust it.
    const inline = (file) => JSON.stringify(fs.readFileSync(file, 'utf8'))
    const userNpmrc = writeNpmrc(fixture, 'user.npmrc', [
      `registry=${fixture.registryUrl}/`,
      `ca[]=${inline(TLS_CA_FILE)}`,
      `ca[]=${inline(path.join(path.dirname(TLS_CA_FILE), 'server.crt'))}`,
    ])

    const result = await runEntry(fixture, 'bin/pnpm.mjs', ['--version'], userNpmConfig(userNpmrc))
    assertDownloaded(fixture, result)
  })

  it('trusts the CA the workspace .npmrc names for the registry it names', { skip: ADDS_DEFAULT_CAS }, async () => {
    const fixture = await createFixture({ tls: true })
    fs.writeFileSync(path.join(fixture.projectDir, 'pnpm-workspace.yaml'), '')
    fs.mkdirSync(path.join(fixture.projectDir, 'certs'))
    fs.copyFileSync(TLS_CA_FILE, path.join(fixture.projectDir, 'certs/ca.pem'))
    // Relative to the .npmrc, as pnpm reads it, not to the directory pnpm runs in.
    writeNpmrc(fixture, 'project/.npmrc', [
      `registry=${fixture.registryUrl}/`,
      'cafile=certs/ca.pem',
    ])
    const userNpmrc = writeNpmrc(fixture, 'user.npmrc', [])
    const packageDir = path.join(fixture.projectDir, 'packages/a')
    fs.mkdirSync(packageDir, { recursive: true })

    const result = await runEntry({ ...fixture, projectDir: packageDir }, 'bin/pnpm.mjs', ['--version'], userNpmConfig(userNpmrc))
    assertDownloaded(fixture, result)
  })

  it('trusts the CA the workspace .npmrc scopes to the registry', { skip: ADDS_DEFAULT_CAS }, async () => {
    const fixture = await createFixture({ tls: true })
    fs.writeFileSync(path.join(fixture.projectDir, 'pnpm-workspace.yaml'), '')
    writeNpmrc(fixture, 'project/.npmrc', [
      `registry=${fixture.registryUrl}/`,
      `${nerfDart(fixture.registryUrl)}:ca=${JSON.stringify(fs.readFileSync(TLS_CA_FILE, 'utf8'))}`,
    ])
    const userNpmrc = writeNpmrc(fixture, 'user.npmrc', [])

    const result = await runEntry(fixture, 'bin/pnpm.mjs', ['--version'], userNpmConfig(userNpmrc))
    assertDownloaded(fixture, result)
  })

  it('does not trust a CA scoped to another registry', { skip: ADDS_DEFAULT_CAS }, async () => {
    const fixture = await createFixture({ tls: true })
    const userNpmrc = writeNpmrc(fixture, 'user.npmrc', [
      `registry=${fixture.registryUrl}/`,
      `//registry.example.com/:cafile=${TLS_CA_FILE}`,
    ])

    const result = await runEntry(fixture, 'bin/pnpm.mjs', ['--version'], userNpmConfig(userNpmrc))
    assert.match(result.stderr, /Could not reach/)
    assert.deepEqual(fixture.requests, [])
  })

  it('does not let a project .npmrc add a CA to an unchecked download', { skip: ADDS_DEFAULT_CAS }, async () => {
    const fixture = await createFixture({ tls: true })
    writeNpmrc(fixture, 'project/.npmrc', [`cafile=${TLS_CA_FILE}`])
    const userNpmrc = writeNpmrc(fixture, 'user.npmrc', [`registry=${fixture.registryUrl}/`])

    const result = await runEntry(fixture, 'bin/pnpm.mjs', ['--version'], {
      ...userNpmConfig(userNpmrc),
      COREPACK_INTEGRITY_KEYS: '0',
    })
    assert.match(result.stderr, /Could not reach/)
    assert.deepEqual(fixture.requests, [])
  })

  it('reports a disabled network instead of reaching for one', async () => {
    const fixture = await createFixture()

    const result = await runEntry(fixture, 'bin/pnpm.mjs', ['--version'], { COREPACK_ENABLE_NETWORK: '0' })
    assert.notEqual(result.status, 0)
    assert.match(result.stderr, /Network access is disabled/)
  })
})

// Asynchronous on purpose: the registry runs in this process, so blocking on
// the child would deadlock the download. A variable `env` sets to `undefined`
// is left out of the child's environment.
function runEntry (fixture, entry, args, env) {
  const childEnv = {
    ...process.env,
    COREPACK_NPM_REGISTRY: fixture.registryUrl,
    COREPACK_INTEGRITY_KEYS: INTEGRITY_KEYS,
    ...env,
  }
  for (const [name, value] of Object.entries(childEnv)) {
    if (value === undefined) delete childEnv[name]
  }
  const child = spawn(process.execPath, [path.join(fixture.dir, entry), ...args], {
    cwd: fixture.projectDir,
    env: childEnv,
  })

  let stdout = ''
  let stderr = ''
  child.stdout.setEncoding('utf8').on('data', (chunk) => { stdout += chunk })
  child.stderr.setEncoding('utf8').on('data', (chunk) => { stderr += chunk })

  return new Promise((resolve, reject) => {
    child.on('error', reject)
    child.on('close', (status) => { resolve({ status, stdout, stderr }) })
  })
}

/**
 * A wrapper directory holding what Corepack unpacks — the entry points, the
 * manifest, and the `dist/` payload the downloader travels in — plus a registry
 * serving the platform package.
 */
async function createFixture (registryOptions = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-corepack-entry-'))
  after(() => fs.rmSync(dir, { force: true, recursive: true }))

  for (const file of WRAPPER_FILES) {
    fs.mkdirSync(path.dirname(path.join(dir, file)), { recursive: true })
    fs.copyFileSync(path.join(WRAPPER_DIR, file), path.join(dir, file))
  }
  for (const [name, packageDir] of downloaderPackages()) {
    fs.cpSync(packageDir, path.join(dir, BUNDLED_MODULES, name), { dereference: true, recursive: true })
  }
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({ name: 'pnpm', version: VERSION }))
  const projectDir = path.join(dir, 'project')
  fs.mkdirSync(projectDir)

  const registry = await startRegistry({ payload: FAKE_BINARY, ...registryOptions })
  after(registry.close)

  return { dir, projectDir, registryUrl: registry.url, requests: registry.requests, closeRegistry: registry.close }
}

function writeNpmrc (fixture, file, lines) {
  const npmrcPath = path.join(fixture.dir, file)
  fs.writeFileSync(npmrcPath, `${lines.join('\n')}\n`)
  return npmrcPath
}

/**
 * An environment whose only npm configuration is `userNpmrc`, with no registry
 * named by Corepack or by an environment variable.
 */
function userNpmConfig (userNpmrc) {
  return {
    ...Object.fromEntries(ENV_NPM_CONFIG.map((name) => [name, undefined])),
    COREPACK_NPM_REGISTRY: undefined,
    // A CA the machine running the tests trusts must not make a TLS test pass.
    NODE_EXTRA_CA_CERTS: undefined,
    NODE_USE_SYSTEM_CA: undefined,
    npm_config_userconfig: userNpmrc,
  }
}

/**
 * The download went through: get-pnpm places the binary only once its
 * signature and checksum are verified and it is extracted. Where the stand-in
 * can be spawned, the binary also has to have run.
 */
function assertDownloaded (fixture, result) {
  assert.deepEqual(new Set(fixture.requests.map(({ url }) => url)), new Set([
    `/${packageName}/${VERSION}`,
    `/${packageName}/-/${VERSION}.tgz`,
  ]))
  assert.ok(fs.existsSync(path.join(fixture.dir, DOWNLOADED_BINARY)), result.stderr)
  if (!SPAWNS_THE_BINARY) {
    assert.equal(result.status, 0, result.stderr)
    assert.match(result.stdout, /ran: --version/)
  }
}

function nerfDart (registryUrl) {
  return `//${new URL(registryUrl).host}/`
}

/**
 * The installed get-pnpm and every package it depends on, by name: what the
 * release bundles into `dist/node_modules`, side by side.
 */
function downloaderPackages (name = 'get-pnpm', from = WRAPPER_DIR, found = new Map()) {
  if (found.has(name)) {
    return found
  }
  const lookup = createRequire(path.join(from, 'package.json')).resolve.paths(name)
  const packageDir = fs.realpathSync(lookup.map((dir) => path.join(dir, name)).find((dir) => fs.existsSync(dir)))
  found.set(name, packageDir)
  const { dependencies = {} } = JSON.parse(fs.readFileSync(path.join(packageDir, 'package.json'), 'utf8'))
  for (const dependency of Object.keys(dependencies)) {
    downloaderPackages(dependency, packageDir, found)
  }
  return found
}
