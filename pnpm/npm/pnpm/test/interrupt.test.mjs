import assert from 'node:assert/strict'
import { spawn, spawnSync } from 'node:child_process'
import { once } from 'node:events'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { test } from 'node:test'
import { URL } from 'node:url'

const unix = { skip: process.platform === 'win32' && 'requires Unix signal delivery' }

for (const signal of ['SIGINT', 'SIGTERM']) {
  test(`the native binary receives ${signal} sent to the wrapper PID`, {
    skip: unix.skip || process.platform === 'os400' || typeof process.execve !== 'function',
    timeout: 10000,
  }, async t => {
    const directory = writeFixture(t)
    const binary = path.join(directory, 'pnpm-native')
    fs.writeFileSync(binary, `#!${process.execPath}
process.on('${signal}', () => process.exit(42))
console.log(process.pid)
setInterval(() => {}, 1000)
`)
    const child = spawn(process.execPath, [path.join(directory, 'bin', 'pnpm.mjs')], {
      detached: true,
      stdio: ['ignore', 'pipe', 'inherit'],
    })
    t.after(() => {
      if (child.exitCode != null || child.signalCode != null) return
      try {
        process.kill(-child.pid, 'SIGKILL')
      } catch (error) {
        if (error.code !== 'ESRCH') throw error
      }
    })
    const exited = once(child, 'exit')
    const [ready] = await once(child.stdout, 'data')
    assert.equal(Number(ready), child.pid)
    child.kill(signal)
    assert.deepEqual(await exited, [42, null])
  })
}

test('an interrupt before the native binary is spawned prevents it from running', unix, t => {
  const directory = writeFixture(t)
  const preload = path.join(directory, 'interrupt.cjs')
  fs.writeFileSync(preload, `
process.execve = undefined
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
  const result = runWrapper(directory, ['--require', preload])
  assert.equal(result.status, 130, result.stderr)
  assert.equal(result.stdout, '')
})

test('the wrapper can launch the binary without a system shell', unix, t => {
  const directory = writeFixture(t)
  const preload = path.join(directory, 'missing-shell.cjs')
  fs.writeFileSync(preload, `
process.execve = undefined
const childProcess = require('node:child_process')
const original = childProcess.spawn
childProcess.spawn = (file, ...args) => original(
  file.endsWith('/bin/sh') ? ${JSON.stringify(path.join(directory, 'missing-shell'))} : file,
  ...args
)
require('node:module').syncBuiltinESMExports()
`)
  const result = runWrapper(directory, ['--require', preload])
  assert.equal(result.status, 0, result.stderr)
  assert.equal(result.stdout, 'native binary ran\n')
})

test('the wrapper reports a non-executable native binary', unix, t => {
  const directory = writeFixture(t)
  fs.chmodSync(path.join(directory, 'pnpm-native'), 0o644)
  const result = runWrapper(directory)
  assert.equal(result.status, 1)
  assert.match(result.stderr, /Could not run the pnpm binary at .*pnpm-native: .*EACCES/)
})

function writeFixture (t) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-wrapper-interrupt-'))
  t.after(() => fs.rmSync(directory, { force: true, recursive: true }))
  fs.copyFileSync(new URL('../native-binary.mjs', import.meta.url), path.join(directory, 'native-binary.mjs'))
  fs.mkdirSync(path.join(directory, 'bin'))
  fs.copyFileSync(new URL('../bin/pnpm.mjs', import.meta.url), path.join(directory, 'bin', 'pnpm.mjs'))
  fs.writeFileSync(path.join(directory, 'package.json'), '{"name":"pnpm","version":"99.0.0"}')
  const shell = process.platform === 'android' ? '/system/bin/sh' : '/bin/sh'
  fs.writeFileSync(path.join(directory, 'pnpm-native'), `#!${shell}\necho "native binary ran"\n`, { mode: 0o755 })
  return directory
}

function runWrapper (directory, nodeArgs = []) {
  const result = spawnSync(process.execPath, [...nodeArgs, path.join(directory, 'bin', 'pnpm.mjs')], {
    encoding: 'utf8',
    timeout: 15000,
  })
  assert.ifError(result.error)
  return result
}
