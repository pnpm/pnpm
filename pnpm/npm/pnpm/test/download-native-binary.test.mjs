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
import { VERSION, binFile, digest, packageName, startRegistry } from './registry-fixture.mjs'

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
    assert.ok(registry.requestedPaths.includes(`/${packageName}/-/${VERSION}.tgz`))
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

  it('reports a tarball that does not carry the binary', async () => {
    const destPath = await givenRegistry({})

    await assert.rejects(download(destPath, { binFile: 'not-there' }), /contains no not-there/)
  })

  // Whether this run's copy replaces the other one or is dropped is the
  // platform's call — on Windows the rename fails once the winner is executing
  // it. Either way the download reports success and cleans up after itself.
  it('accepts a binary a concurrent run already placed', async () => {
    const destPath = await givenRegistry({})
    fs.writeFileSync(destPath, `${PAYLOAD}, placed by the winner`)

    await download(destPath)

    assert.match(fs.readFileSync(destPath, 'utf8'), new RegExp(`^${PAYLOAD}`))
    assert.deepEqual(leftovers(destPath), [path.basename(destPath)])
  })

  it('refuses to reach the network when the environment forbids it', async () => {
    const destPath = await givenRegistry({})
    process.env.COREPACK_ENABLE_NETWORK = '0'
    cleanups.push(() => { delete process.env.COREPACK_ENABLE_NETWORK })

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
  process.env.COREPACK_NPM_REGISTRY = registry.url
  cleanups.push(() => { delete process.env.COREPACK_NPM_REGISTRY })
  return registry
}

function destIn () {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-native-download-'))
  cleanups.push(() => fs.rmSync(dir, { force: true, recursive: true }))
  return path.join(dir, process.platform === 'win32' ? 'pnpm-native.exe' : 'pnpm-native')
}
