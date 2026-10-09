import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'

const SCRIPT = path.join(path.dirname(fileURLToPath(import.meta.url)), 'cargo-dylint.mjs')

function stubDirs (context, layout) {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-cargo-dylint-')))
  context.after(() => fs.rmSync(root, { recursive: true, force: true }))
  const record = path.join(root, 'invocations')
  const dirs = {}
  for (const [dir, names] of Object.entries(layout)) {
    dirs[dir] = path.join(root, dir)
    fs.mkdirSync(dirs[dir])
    for (const name of names) {
      fs.writeFileSync(
        path.join(dirs[dir], name),
        `#!/bin/sh\necho "${dir}/${name} $* PATH=$PATH" >> '${record}'\n`,
        { mode: 0o755 }
      )
    }
  }
  return { dirs, record }
}

function runHelper (searchPath) {
  return spawnSync(process.execPath, [SCRIPT, '--all'], {
    encoding: 'utf8',
    env: { PATH: [...searchPath, '/usr/bin', '/bin'].join(path.delimiter) },
  })
}

test('runs the cargo beside rustup, ahead of a toolchain cargo earlier on PATH', { skip: process.platform === 'win32' }, (context) => {
  const { dirs, record } = stubDirs(context, {
    toolchain: ['cargo'],
    rustupOnly: ['rustup'],
    proxies: ['rustup', 'cargo'],
  })

  const result = runHelper([dirs.toolchain, dirs.rustupOnly, dirs.proxies])

  assert.equal(result.status, 0, result.stderr)
  const [invocation, ...rest] = fs.readFileSync(record, 'utf8').trim().split('\n')
  assert.deepEqual(rest, [])
  assert.ok(invocation.startsWith('proxies/cargo dylint --all PATH='), invocation)
  assert.equal(invocation.split('PATH=')[1].split(path.delimiter)[0], dirs.proxies)
})

test('fails without a directory that holds both rustup and cargo', { skip: process.platform === 'win32' }, (context) => {
  const { dirs, record } = stubDirs(context, {
    toolchain: ['cargo'],
    rustupOnly: ['rustup'],
  })

  const result = runHelper([dirs.toolchain, dirs.rustupOnly])

  assert.equal(result.status, 1)
  assert.match(result.stderr, /needs rustup/)
  assert.equal(fs.existsSync(record), false)
})
