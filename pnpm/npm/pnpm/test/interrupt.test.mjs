import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { test } from 'node:test'

test('an interrupt before the native binary is spawned prevents it from running', {
  skip: process.platform === 'win32' && 'requires Unix signal delivery',
}, t => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-wrapper-interrupt-'))
  t.after(() => fs.rmSync(directory, { force: true, recursive: true }))
  fs.copyFileSync(new URL('../native-binary.mjs', import.meta.url), path.join(directory, 'native-binary.mjs'))
  fs.mkdirSync(path.join(directory, 'bin'))
  fs.copyFileSync(new URL('../bin/pnpm.mjs', import.meta.url), path.join(directory, 'bin', 'pnpm.mjs'))
  fs.writeFileSync(path.join(directory, 'package.json'), '{"name":"pnpm","version":"99.0.0"}')
  fs.writeFileSync(path.join(directory, 'pnpm-native'), '#!/bin/sh\necho "native binary ran"\n', { mode: 0o755 })

  const preload = path.join(directory, 'interrupt.cjs')
  fs.writeFileSync(preload, `
const childProcess = require('node:child_process')
for (const method of ['spawn', 'spawnSync']) {
  const original = childProcess[method]
  childProcess[method] = (...args) => {
    process.kill(process.pid, 'SIGINT')
    return original(...args)
  }
}
require('node:module').syncBuiltinESMExports()
`)

  const result = spawnSync(process.execPath, ['--require', preload, path.join(directory, 'bin', 'pnpm.mjs')], {
    encoding: 'utf8',
    timeout: 15000,
  })
  assert.ifError(result.error)
  assert.equal(result.status, 130, result.stderr)
  assert.equal(result.stdout, '')
})
