// Exercises the Corepack entry points (`bin/pnpm.mjs`, `bin/pnpx.mjs`) the way
// Corepack uses them: a wrapper directory without the `@pnpm/exe.<target>`
// dependency, against a registry that serves a stand-in for the native binary.
//
// Skipped on Windows, where the stand-in — a shell script, so it can report how
// it was called — is not executable. Everything up to spawning that stand-in is
// covered on every platform by download-native-binary.test.mjs.
import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { after, describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

import { VERSION, binFile, digest, packageName, startRegistry } from './registry-fixture.mjs'

const WRAPPER_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const WRAPPER_FILES = [
  'native-binary.mjs',
  'bin/pnpm.mjs',
  'bin/pnpx.mjs',
  'bin/download-native-binary.mjs',
]
// A stand-in for the 40 MB native binary: reports how it was called, and turns
// a `fail` argument into a non-zero exit so exit codes can be asserted.
const FAKE_BINARY = `#!/bin/sh
if [ "$1" = fail ]; then exit 3; fi
echo "ran: $*"
`

const SKIP_ON_WINDOWS = process.platform === 'win32' &&
  'the stand-in for the native binary is a shell script; download-native-binary.test.mjs covers everything up to spawning it'

describe('corepack entry point', { skip: SKIP_ON_WINDOWS }, () => {
  it('downloads the native binary on first use, then reuses it', async () => {
    const fixture = await createFixture()

    const first = await runEntry(fixture, 'bin/pnpm.mjs', ['--version'])
    assert.equal(first.status, 0, first.stderr)
    assert.match(first.stdout, /ran: --version/)
    assert.match(first.stderr, /Downloading the pnpm 12\.0\.0-test\.0 binary/)
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

  it('reports a refused download instead of running something else', async () => {
    const fixture = await createFixture({ integrity: () => digest('sha512', 'not the tarball') })

    const result = await runEntry(fixture, 'bin/pnpm.mjs', ['--version'])
    assert.notEqual(result.status, 0)
    assert.match(result.stderr, /Integrity check failed/)
    assert.equal(fs.existsSync(path.join(fixture.dir, 'pnpm-native')), false)
  })
})

// Asynchronous on purpose: the registry runs in this process, so blocking on
// the child would deadlock the download.
function runEntry (fixture, entry, args, env) {
  const child = spawn(process.execPath, [path.join(fixture.dir, entry), ...args], {
    encoding: 'utf8',
    env: {
      ...process.env,
      COREPACK_NPM_REGISTRY: fixture.registryUrl,
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
 * A wrapper directory holding only what Corepack unpacks — the entry points and
 * a manifest pinning the platform package — plus a registry serving that
 * package.
 */
async function createFixture (registryOptions) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-corepack-entry-'))
  after(() => fs.rmSync(dir, { force: true, recursive: true }))

  for (const file of WRAPPER_FILES) {
    fs.mkdirSync(path.dirname(path.join(dir, file)), { recursive: true })
    fs.copyFileSync(path.join(WRAPPER_DIR, file), path.join(dir, file))
  }
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({
    name: 'pnpm',
    version: VERSION,
    optionalDependencies: { [packageName]: VERSION },
  }))

  const registry = await startRegistry({ payload: FAKE_BINARY, ...registryOptions })
  after(registry.close)

  return { dir, registryUrl: registry.url, closeRegistry: registry.close }
}
