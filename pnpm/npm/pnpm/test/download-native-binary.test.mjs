// The download the Corepack entry point falls back on: what it fetches, what
// it refuses, and what it leaves on disk. Runs on every platform — nothing here
// executes the downloaded file, which is what confines the entry-point tests to
// Unix.
import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { afterEach, describe, it } from 'node:test'

import { downloadNativeBinary } from '../bin/download-native-binary.mjs'
import { VERSION, binFile, digest, packageName, shasumOf, startRegistry } from './registry-fixture.mjs'

const PAYLOAD = 'the native binary, supposedly'

const cleanups = []

afterEach(async () => {
  while (cleanups.length > 0) await cleanups.pop()()
})

describe('native binary download', () => {
  it('extracts the binary out of the published tarball', async () => {
    const destPath = await givenRegistry({})

    await download(destPath)

    assert.equal(fs.readFileSync(destPath, 'utf8'), PAYLOAD)
    assert.deepEqual(leftovers(destPath), [path.basename(destPath)])
  })

  it('marks the binary executable', { skip: process.platform === 'win32' }, async () => {
    const destPath = await givenRegistry({})

    await download(destPath)

    assert.equal(fs.statSync(destPath).mode & 0o111, 0o111)
  })

  it('follows the tarball URL onto the configured registry', async () => {
    // What a registry proxying npm hands back: npm's own URL, which must not be
    // fetched from npm.
    const registry = await givenRegistryServer({
      tarballUrl: `https://registry.npmjs.org/${packageName}/-/${VERSION}.tgz`,
    })
    const destPath = destIn()

    await download(destPath)

    assert.equal(fs.readFileSync(destPath, 'utf8'), PAYLOAD)
    assert.ok(registry.requests.some(({ path: requested }) => requested === `/${packageName}/-/${VERSION}.tgz`))
  })

  it('refuses a tarball that does not match the published checksum', async () => {
    const destPath = await givenRegistry({ integrity: () => digest('sha512', 'not the tarball') })

    await assert.rejects(download(destPath), /Integrity check failed/)
    assert.deepEqual(leftovers(destPath), [])
  })

  it('refuses a tarball published with only a SHA-1 checksum', async () => {
    const destPath = await givenRegistry({ integrity: (tarball) => digest('sha1', tarball) })

    await assert.rejects(download(destPath), /published no usable checksum/)
    assert.deepEqual(leftovers(destPath), [])
  })

  it('checks the strongest published checksum, not the first one', async () => {
    const destPath = await givenRegistry({
      integrity: (tarball) => `${digest('sha256', tarball)} ${digest('sha512', 'not the tarball')}`,
    })

    await assert.rejects(download(destPath), /expected sha512-/)
  })

  it('falls back to the legacy shasum of a registry that publishes no integrity', async () => {
    const destPath = await givenRegistry({ integrity: () => undefined, shasum: shasumOf })

    await download(destPath)

    assert.equal(fs.readFileSync(destPath, 'utf8'), PAYLOAD)
  })

  it('refuses a tarball that does not match the legacy shasum', async () => {
    const destPath = await givenRegistry({
      integrity: () => undefined,
      shasum: () => shasumOf('not the tarball'),
    })

    await assert.rejects(download(destPath), /Integrity check failed/)
    assert.deepEqual(leftovers(destPath), [])
  })

  it('refuses a tarball published with no checksum at all', async () => {
    const destPath = await givenRegistry({ integrity: () => undefined })

    await assert.rejects(download(destPath), /published no checksum/)
  })

  it('reports a tarball that does not carry the binary', async () => {
    const destPath = await givenRegistry({})

    await assert.rejects(download(destPath, { binFile: 'not-there' }), /contains no not-there/)
  })

  it('sends the configured credentials to the registry, and nowhere else', async () => {
    // The tarball comes from a second origin, as it does for a registry that
    // offloads downloads to a CDN.
    const registry = await givenRegistryServer({ tarballElsewhere: true })
    setEnv('COREPACK_NPM_TOKEN', 'a-token')

    await download(destIn())

    assert.equal(registry.requests[0].headers.authorization, 'Bearer a-token')
    assert.equal(registry.tarballRequests[0].headers.authorization, undefined)
  })

  // Whether a run's copy replaces the other one or is dropped is the platform's
  // call — on Windows the rename fails once the winner is executing it. Either
  // way the download reports success and cleans up after itself.
  it('accepts a binary a concurrent run already placed', async () => {
    const destPath = await givenRegistry({})
    fs.writeFileSync(destPath, `${PAYLOAD}, placed by the winner`)

    await download(destPath)

    assert.match(fs.readFileSync(destPath, 'utf8'), new RegExp(`^${PAYLOAD}`))
    assert.deepEqual(leftovers(destPath), [path.basename(destPath)])
  })

  it('lets concurrent downloads of the same binary all succeed', async () => {
    const destPath = await givenRegistry({})

    await Promise.all(Array.from({ length: 4 }, () => download(destPath)))

    assert.equal(fs.readFileSync(destPath, 'utf8'), PAYLOAD)
    assert.deepEqual(leftovers(destPath), [path.basename(destPath)])
  })

  it('refuses to reach the network when the environment forbids it', async () => {
    const destPath = await givenRegistry({})
    setEnv('COREPACK_ENABLE_NETWORK', '0')

    await assert.rejects(download(destPath), /Network access is disabled/)
  })
})

function download (destPath, overrides) {
  return downloadNativeBinary({ packageName, version: VERSION, binFile, destPath, ...overrides })
}

/** Every file in the destination directory, so temp files can't hide. */
function leftovers (destPath) {
  return fs.readdirSync(path.dirname(destPath)).sort()
}

async function givenRegistry (options) {
  await givenRegistryServer(options)
  return destIn()
}

async function givenRegistryServer (options) {
  const registry = await startRegistry({ payload: PAYLOAD, ...options })
  cleanups.push(registry.close)
  setEnv('COREPACK_NPM_REGISTRY', registry.url)
  return registry
}

function destIn () {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-native-download-'))
  cleanups.push(() => fs.rmSync(dir, { force: true, recursive: true }))
  return path.join(dir, process.platform === 'win32' ? 'pnpm-native.exe' : 'pnpm-native')
}

/** Set an environment variable for one test, restoring what was there before. */
function setEnv (name, value) {
  const previous = process.env[name]
  process.env[name] = value
  cleanups.push(() => {
    if (previous === undefined) {
      delete process.env[name]
    } else {
      process.env[name] = previous
    }
  })
}
