// Exercises the Corepack entry points (`bin/pnpm.mjs`, `bin/pnpx.mjs`) the way
// Corepack uses them: a wrapper directory without the `@pnpm/exe.<target>`
// dependency, against a registry that serves a stand-in for the native binary.
//
// Skipped on Windows, where the stand-in (a shell script) is not executable.
import assert from 'node:assert/strict'
import { Buffer } from 'node:buffer'
import { spawn } from 'node:child_process'
import { createHash } from 'node:crypto'
import fs from 'node:fs'
import http from 'node:http'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { after, describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import zlib from 'node:zlib'

import { getBinCandidates, splitBinSpecifier } from '../native-binary.mjs'

const WRAPPER_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const WRAPPER_FILES = [
  'native-binary.mjs',
  'bin/pnpm.mjs',
  'bin/pnpx.mjs',
  'bin/download-native-binary.mjs',
]
const VERSION = '12.0.0-test.0'
const { packageName, binFile } = splitBinSpecifier(getBinCandidates()[0])
// A stand-in for the 40 MB native binary: reports how it was called, and turns
// a `fail` argument into a non-zero exit so exit codes can be asserted.
const FAKE_BINARY = `#!/bin/sh
if [ "$1" = fail ]; then exit 3; fi
echo "ran: $*"
`

describe('corepack entry point', { skip: process.platform === 'win32' }, () => {
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

  it('refuses a tarball that does not match the published checksum', async () => {
    const fixture = await createFixture({ corruptIntegrity: true })

    const result = await runEntry(fixture, 'bin/pnpm.mjs', ['--version'])
    assert.notEqual(result.status, 0)
    assert.match(result.stderr, /Integrity check failed/)
    assert.equal(fs.existsSync(path.join(fixture.dir, 'pnpm-native')), false)
  })

  it('reports a disabled network instead of hanging', async () => {
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
async function createFixture ({ corruptIntegrity = false } = {}) {
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

  const tarball = zlib.gzipSync(tarArchive(`package/${binFile}`, Buffer.from(FAKE_BINARY)))
  const integrity = corruptIntegrity
    ? `sha512-${createHash('sha512').update('not the tarball').digest('base64')}`
    : `sha512-${createHash('sha512').update(tarball).digest('base64')}`
  const registry = await startRegistry({ tarball, integrity })
  after(registry.close)

  return { dir, registryUrl: registry.url, closeRegistry: registry.close }
}

function startRegistry ({ tarball, integrity }) {
  const tarballPath = `/${packageName}/-/${VERSION}.tgz`
  const server = http.createServer((req, res) => {
    if (req.url === `/${packageName.replaceAll('/', '%2F')}/${VERSION}`) {
      res.writeHead(200, { 'content-type': 'application/json' })
      res.end(JSON.stringify({
        name: packageName,
        version: VERSION,
        dist: { integrity, tarball: `http://127.0.0.1:${server.address().port}${tarballPath}` },
      }))
    } else if (req.url === tarballPath) {
      res.writeHead(200, { 'content-type': 'application/octet-stream' })
      res.end(tarball)
    } else {
      res.writeHead(404).end()
    }
  })

  return new Promise((resolve) => {
    server.listen(0, '127.0.0.1', () => {
      resolve({
        url: `http://127.0.0.1:${server.address().port}`,
        close: () => new Promise((closed) => { server.close(closed) }),
      })
    })
  })
}

/** A single-file ustar archive, terminated by the two empty blocks. */
function tarArchive (name, content) {
  const header = Buffer.alloc(512)
  header.write(name, 0, 100)
  header.write('000755 \0', 100, 8)
  header.write('000000 \0', 108, 8)
  header.write('000000 \0', 116, 8)
  header.write(`${content.length.toString(8).padStart(11, '0')} `, 124, 12)
  header.write('00000000000 ', 136, 12)
  header.write('        ', 148, 8)
  header.write('0', 156, 1)
  header.write('ustar\x0000', 257, 8)
  const checksum = header.reduce((sum, byte) => sum + byte, 0)
  header.write(`${checksum.toString(8).padStart(6, '0')}\0 `, 148, 8)

  const padding = Buffer.alloc((512 - (content.length % 512)) % 512)
  return Buffer.concat([header, content, padding, Buffer.alloc(1024)])
}
