import assert from 'node:assert/strict'
import { Buffer } from 'node:buffer'
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'

import { getBinCandidates, splitBinSpecifier } from '../native-binary.mjs'

const WRAPPER_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const BIN_NAMES = ['pnpm', 'pn', 'pnpx', 'pnx']

// The bins hand over to `bin/pnpm.mjs` when a shell runs them.
for (const entry of ['bin/pnpm.mjs', 'bin/pnpx.mjs']) {
  test(`${entry} points WebContainer users to @pnpm/wasm without downloading a native binary`, t => {
    const fixture = createFixture(t)
    const result = runEntry(fixture, entry, ['--version'])
    assert.equal(result.status, 1)
    assert.match(result.stderr, /Install @pnpm\/wasm instead/)
    assert.equal(fs.existsSync(path.join(fixture, 'pnpm-native')), false)
  })
}

test('WebContainer installation leaves the placeholder bins in place', t => {
  const fixture = createFixture(t)
  const installed = runEntry(fixture, 'install.js', [], { npm_lifecycle_event: 'preinstall' })
  assert.equal(installed.status, 0, installed.stderr)
  for (const name of BIN_NAMES) {
    assert.deepEqual(fs.readFileSync(path.join(fixture, name)), fs.readFileSync(path.join(WRAPPER_DIR, name)))
  }
})

test('the native installer keeps a byte-identical binary instead of a runtime-selection launcher', {
  skip: getBinCandidates().length === 0 && 'No native package for this host',
}, t => {
  const fixture = createFixture(t)
  const { packageName, binFile } = splitBinSpecifier(getBinCandidates()[0])
  const nativeDirectory = path.join(fixture, 'node_modules', packageName)
  fs.mkdirSync(nativeDirectory, { recursive: true })
  fs.writeFileSync(path.join(nativeDirectory, 'package.json'), JSON.stringify({ name: packageName, version: '99.0.0' }))
  const binary = Buffer.from('native payload must remain byte-identical\0\xff')
  fs.writeFileSync(path.join(nativeDirectory, binFile), binary, { mode: 0o755 })
  const result = runEntry(fixture, 'install.js', [], {}, false)
  assert.equal(result.status, 0, result.stderr)
  for (const name of BIN_NAMES) assert.deepEqual(fs.readFileSync(path.join(fixture, name)), binary)
})

function createFixture (t) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-webcontainer-wrapper-'))
  t.after(() => fs.rmSync(directory, { force: true, recursive: true }))
  for (const file of [...BIN_NAMES, 'install.js', 'native-binary.mjs']) {
    fs.copyFileSync(path.join(WRAPPER_DIR, file), path.join(directory, file))
  }
  fs.cpSync(path.join(WRAPPER_DIR, 'bin'), path.join(directory, 'bin'), { recursive: true })
  fs.writeFileSync(path.join(directory, 'package.json'), JSON.stringify({
    name: 'pnpm', version: '99.0.0', type: 'module', optionalDependencies: {},
  }))
  return directory
}

function runEntry (directory, entry, args = [], env = {}, webcontainer = true) {
  const file = path.join(directory, entry)
  const marker = webcontainer ? "Object.defineProperty(process.versions, 'webcontainer', { value: 'test' });" : ''
  const code = `import { pathToFileURL } from 'node:url'; ${marker} process.argv = ${JSON.stringify([process.execPath, file, ...args])}; await import(pathToFileURL(${JSON.stringify(file)}));`
  return spawnSync(process.execPath, ['--input-type=module', '-e', code], {
    encoding: 'utf8', timeout: 15000, env: { ...process.env, npm_execpath: undefined, npm_lifecycle_event: 'preinstall', ...env },
  })
}
