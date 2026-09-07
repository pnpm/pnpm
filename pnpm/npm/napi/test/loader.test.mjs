import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'

const loaderPath = path.resolve(fileURLToPath(import.meta.url), '../../index.js')

test('a big-endian POWER host is not pointed at the released addon', (t) => {
  const message = loadOn(t, { arch: 'ppc64', endianness: 'BE' })

  assert.match(message, /No addon is published for this host/)
  // Only a little-endian POWER addon is released, so naming a platform package
  // here would tell the user to install one that can never load.
  assert.doesNotMatch(message, /@pnpm\/napi\./)
})

test('a little-endian POWER host is still pointed at its platform package', (t) => {
  const message = loadOn(t, { arch: 'ppc64', endianness: 'LE' })

  assert.match(message, /Install the matching @pnpm\/napi platform package/)
})

/**
 * Load the addon loader in a child process presenting itself as a Linux host of
 * the given architecture and byte order, from a directory holding nothing but
 * the loader so that neither a platform package nor a locally built artifact
 * resolves.
 *
 * @param {import('node:test').TestContext} t The test, for cleanup.
 * @param {{ arch: string, endianness: 'LE' | 'BE' }} host The host to present.
 * @returns {string} The message of the load failure it reports.
 */
function loadOn (t, host) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-napi-loader-'))
  t.after(() => { fs.rmSync(dir, { recursive: true, force: true }) })
  const loaderCopy = path.join(dir, 'index.js')
  fs.copyFileSync(loaderPath, loaderCopy)

  const script = `
    const os = require('node:os')
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
    Object.defineProperty(process, 'arch', { value: ${JSON.stringify(host.arch)}, configurable: true })
    Object.defineProperty(os, 'endianness', { value: () => ${JSON.stringify(host.endianness)}, configurable: true })
    try {
      require(${JSON.stringify(loaderCopy)})
      process.stdout.write('the loader unexpectedly succeeded')
    } catch (err) {
      process.stdout.write(err.message)
    }
  `
  const env = { ...process.env }
  delete env.PNPM_NAPI_BINARY
  return execFileSync(process.execPath, ['-e', script], { encoding: 'utf8', env })
}
