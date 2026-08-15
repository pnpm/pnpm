// Exercises the Corepack entry points (`bin/pnpm.mjs`, `bin/pnpx.mjs`) the way
// Corepack uses them: a wrapper directory without the `@pnpm/exe.<target>`
// dependency, against a registry that serves a stand-in for the native binary.
//
// The stand-in is a shell script, so the tests that spawn it are Unix-only; the
// download it comes through is get-pnpm's, and tested there.
import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import fs from 'node:fs'
import { createRequire } from 'node:module'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { after, describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

import { INTEGRITY_KEYS, VERSION, binFile, packageName, startRegistry } from './registry-fixture.mjs'

const WRAPPER_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const WRAPPER_FILES = ['native-binary.mjs', 'bin/pnpm.mjs', 'bin/pnpx.mjs']
// Where the published wrapper carries get-pnpm; scripts/bundle-node-gyp.mjs
// puts it there, and the fixture stands in for that.
const BUNDLED_DOWNLOADER = 'dist/node_modules/get-pnpm'
// A stand-in for the 40 MB native binary: reports how it was called, and turns
// a `fail` argument into a non-zero exit so exit codes can be asserted.
const FAKE_BINARY = `#!/bin/sh
if [ "$1" = fail ]; then exit 3; fi
echo "ran: $*"
`

const SKIP_ON_WINDOWS = process.platform === 'win32' &&
  'the stand-in for the native binary is a shell script, which Windows cannot spawn'

describe('corepack entry point', { skip: SKIP_ON_WINDOWS }, () => {
  it('downloads the native binary on first use, then reuses it', async () => {
    const fixture = await createFixture()

    const first = await runEntry(fixture, 'bin/pnpm.mjs', ['--version'])
    assert.equal(first.status, 0, first.stderr)
    assert.match(first.stdout, /ran: --version/)
    assert.match(first.stderr, /Downloading the pnpm 99\.0\.0 binary/)
    assert.ok(fs.existsSync(path.join(fixture.dir, 'pnpm-native')))

    // Nothing but the cached binary can answer once the registry is gone.
    await fixture.closeRegistry()
    const second = await runEntry(fixture, 'bin/pnpm.mjs', ['install'])
    assert.equal(second.status, 0, second.stderr)
    assert.match(second.stdout, /ran: install/)
    assert.doesNotMatch(second.stderr, /Downloading/)
  })

  it('propagates the exit code of the native binary', async () => {
    const fixture = await createFixture()

    assert.equal((await runEntry(fixture, 'bin/pnpm.mjs', ['fail'])).status, 3)
  })

  it('runs `pnpx` as `pnpm dlx`', async () => {
    const fixture = await createFixture()

    const result = await runEntry(fixture, 'bin/pnpx.mjs', ['create-vite', 'app'])
    assert.equal(result.status, 0, result.stderr)
    assert.match(result.stdout, /ran: dlx create-vite app/)
  })

  it('prefers the installed platform package over a download', async () => {
    const fixture = await createFixture()
    const installedDir = path.join(fixture.dir, 'node_modules', packageName)
    fs.mkdirSync(installedDir, { recursive: true })
    fs.writeFileSync(path.join(installedDir, 'package.json'), JSON.stringify({ name: packageName, version: VERSION }))
    fs.writeFileSync(path.join(installedDir, binFile), FAKE_BINARY.replace('ran:', 'installed:'), { mode: 0o755 })

    const result = await runEntry(fixture, 'bin/pnpm.mjs', ['--version'])
    assert.equal(result.status, 0, result.stderr)
    assert.match(result.stdout, /installed: --version/)
    assert.equal(fs.existsSync(path.join(fixture.dir, 'pnpm-native')), false)
  })

  it('refuses a download the published checksum does not cover', async () => {
    const fixture = await createFixture({ tamper: true })

    const result = await runEntry(fixture, 'bin/pnpm.mjs', ['--version'])
    assert.notEqual(result.status, 0)
    assert.match(result.stderr, /does not match the checksum/)
    assert.equal(fs.existsSync(path.join(fixture.dir, 'pnpm-native')), false)
  })

  it('refuses a download signed by a key the environment does not name', async () => {
    const fixture = await createFixture()

    const result = await runEntry(fixture, 'bin/pnpm.mjs', ['--version'], {
      COREPACK_INTEGRITY_KEYS: JSON.stringify({ npm: [] }),
    })
    assert.notEqual(result.status, 0)
    assert.match(result.stderr, /unexpected npm key/)
  })

  it('downloads from a registry that publishes no signature when Corepack is told to skip', async () => {
    const fixture = await createFixture({ unsigned: true })

    const refused = await runEntry(fixture, 'bin/pnpm.mjs', ['--version'])
    assert.notEqual(refused.status, 0)
    assert.match(refused.stderr, /carries no npm registry signature/)

    // Corepack's own opt-out, which such a registry already needs for Corepack
    // to have installed this wrapper from it.
    const skipped = await runEntry(fixture, 'bin/pnpm.mjs', ['--version'], { COREPACK_INTEGRITY_KEYS: '0' })
    assert.equal(skipped.status, 0, skipped.stderr)
    assert.match(skipped.stdout, /ran: --version/)
  })

  it('reports a disabled network instead of reaching for one', async () => {
    const fixture = await createFixture()

    const result = await runEntry(fixture, 'bin/pnpm.mjs', ['--version'], { COREPACK_ENABLE_NETWORK: '0' })
    assert.notEqual(result.status, 0)
    assert.match(result.stderr, /Network access is disabled/)
  })
})

// Asynchronous on purpose: the registry runs in this process, so blocking on
// the child would deadlock the download.
function runEntry (fixture, entry, args, env) {
  const child = spawn(process.execPath, [path.join(fixture.dir, entry), ...args], {
    env: {
      ...process.env,
      COREPACK_NPM_REGISTRY: fixture.registryUrl,
      COREPACK_INTEGRITY_KEYS: INTEGRITY_KEYS,
      ...env,
    },
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
  fs.cpSync(downloaderDir(), path.join(dir, BUNDLED_DOWNLOADER), { dereference: true, recursive: true })
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({ name: 'pnpm', version: VERSION }))

  const registry = await startRegistry({ payload: FAKE_BINARY, ...registryOptions })
  after(registry.close)

  return { dir, registryUrl: registry.url, closeRegistry: registry.close }
}

/** The installed get-pnpm, which the release bundles into `dist/`. */
function downloaderDir () {
  const entryPoint = createRequire(import.meta.url).resolve('get-pnpm')
  return path.resolve(entryPoint, '../..')
}
