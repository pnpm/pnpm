import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'

const loaderPath = path.resolve(fileURLToPath(import.meta.url), '../../index.js')

test('a big-endian POWER host is not offered the released addon', (t) => {
  const result = loadOn(t, { arch: 'ppc64', endianness: 'BE' })

  // Both POWER platform packages resolve in there, so loading one would have
  // succeeded. Failing is what proves neither was offered as a candidate.
  assert.match(result, /^FAILED /)
  assert.match(result, /No addon is published for this host/)
  assert.doesNotMatch(result, /@pnpm\/napi\./)
})

test('a little-endian POWER host is offered its platform package', (t) => {
  const result = loadOn(t, { arch: 'ppc64', endianness: 'LE' })

  assert.equal(result, 'LOADED linux-ppc64')
})

/**
 * Load the addon loader in a child process presenting itself as a glibc Linux
 * host of the given architecture and byte order, from a directory carrying a
 * stand-in for each POWER platform package. Offering one is therefore
 * observable: the load succeeds and names the package it came from.
 *
 * @param {import('node:test').TestContext} t The test, for cleanup.
 * @param {{ arch: string, endianness: 'LE' | 'BE' }} host The host to present.
 * @returns {string} `LOADED <triple>`, or `FAILED <message>`.
 */
function loadOn (t, host) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-napi-loader-'))
  t.after(() => { fs.rmSync(dir, { recursive: true, force: true }) })
  const loaderCopy = path.join(dir, 'index.js')
  fs.copyFileSync(loaderPath, loaderCopy)
  for (const triple of ['linux-ppc64', 'linux-ppc64-musl']) {
    const packageDir = path.join(dir, 'node_modules', '@pnpm', `napi.${triple}`)
    fs.mkdirSync(packageDir, { recursive: true })
    fs.writeFileSync(
      path.join(packageDir, 'package.json'),
      JSON.stringify({ name: `@pnpm/napi.${triple}`, version: '0.0.0', main: 'index.js' })
    )
    fs.writeFileSync(path.join(packageDir, 'index.js'), `module.exports = { loadedFrom: ${JSON.stringify(triple)} }\n`)
  }

  // The libc is faked too, so the candidate order does not depend on the
  // platform this test itself runs on.
  const script = `
    const os = require('node:os')
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
    Object.defineProperty(process, 'arch', { value: ${JSON.stringify(host.arch)}, configurable: true })
    Object.defineProperty(process, 'report', {
      value: { getReport: () => ({ header: { glibcVersionRuntime: '2.39' } }) },
      configurable: true,
    })
    Object.defineProperty(os, 'endianness', { value: () => ${JSON.stringify(host.endianness)}, configurable: true })
    try {
      process.stdout.write('LOADED ' + require(${JSON.stringify(loaderCopy)}).loadedFrom)
    } catch (err) {
      process.stdout.write('FAILED ' + err.message)
    }
  `
  const env = { ...process.env }
  delete env.PNPM_NAPI_BINARY
  return execFileSync(process.execPath, ['-e', script], { encoding: 'utf8', env })
}
