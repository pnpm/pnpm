import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'

const WRAPPER_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const ARGS = ['add', 'two words', '$(false)', "quote'", 'back\\slash']
const UNIX = process.platform !== 'win32'

for (const alias of ['pn', 'pnpx', 'pnx']) {
  const expected = alias === 'pn' ? ARGS : ['dlx', ...ARGS]
  test(`${alias} preserves literal arguments and ignores a pnpm earlier on PATH`, t => {
    const fixture = createFixture(t)
    const decoys = path.join(fixture.dir, 'decoys')
    fs.mkdirSync(decoys)
    for (const name of ['pnpm', 'readlink', 'dirname']) {
      fs.writeFileSync(path.join(decoys, name), '#!/bin/sh\necho HIJACKED\n', { mode: 0o755 })
    }
    const result = run(path.join(fixture.wrapper, alias), { PATH: `${decoys}${path.delimiter}${process.env.PATH}` })
    assert.equal(result.status, 0, result.stderr)
    assert.deepEqual(JSON.parse(result.stdout), expected)
  })

  for (const absolute of [false, true]) {
    test(`${alias} resolves ${absolute ? 'absolute' : 'relative'} symlinks to its own package`, { skip: !UNIX }, t => {
      const fixture = createFixture(t)
      const bin = path.join(fixture.dir, 'node_modules', '.bin')
      fs.mkdirSync(bin, { recursive: true })
      fs.writeFileSync(path.join(bin, 'pnpm'), 'throw new Error("HIJACKED")\n')
      const source = path.join(fixture.wrapper, alias)
      const link = path.join(bin, alias)
      fs.symlinkSync(absolute ? source : path.relative(bin, source), link)
      const result = run(link)
      assert.equal(result.status, 0, result.stderr)
      assert.deepEqual(JSON.parse(result.stdout), expected)
    })
  }

  test(`${alias} needs no shell helpers to resolve chained symlinks`, { skip: !UNIX }, t => {
    const fixture = createFixture(t)
    const first = path.join(fixture.dir, 'first')
    const second = path.join(fixture.dir, 'second')
    fs.symlinkSync(path.join(fixture.wrapper, alias), second)
    fs.symlinkSync('second', first)
    const result = run(first, { PATH: '' })
    assert.equal(result.status, 0, result.stderr)
    assert.deepEqual(JSON.parse(result.stdout), expected)
  })

  test(`${alias} preserves the exit status`, t => {
    const fixture = createFixture(t)
    assert.equal(run(path.join(fixture.wrapper, alias), { PNPM_FIXTURE_EXIT: '17' }).status, 17)
  })


}

test('alias entry rejects a symlink cycle without executing an adjacent entry point', { skip: !UNIX }, t => {
  const fixture = createFixture(t)
  const first = path.join(fixture.dir, 'cycle-first')
  fs.symlinkSync('cycle-second', first)
  fs.symlinkSync('cycle-first', path.join(fixture.dir, 'cycle-second'))
  const result = run(first)
  assert.notEqual(result.status, 0)
  assert.match(result.stderr, /ELOOP|ENOENT|Cannot find module/)
})

function run (file, env = {}) {
  return spawnSync(process.execPath, [file, ...ARGS], {
    encoding: 'utf8', timeout: 10_000, env: { ...process.env, ...env },
  })
}

function createFixture (t) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm alias '))
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }))
  const wrapper = path.join(dir, 'package with spaces')
  fs.mkdirSync(path.join(wrapper, 'bin'), { recursive: true })
  for (const file of ['pn', 'pnpx', 'pnx', 'bin/pnpx.mjs']) {
    fs.copyFileSync(path.join(WRAPPER_DIR, file), path.join(wrapper, file))
  }
  fs.writeFileSync(path.join(wrapper, 'package.json'), JSON.stringify({ type: 'module' }))
  fs.writeFileSync(path.join(wrapper, 'bin/pnpm.mjs'), 'console.log(JSON.stringify(process.argv.slice(2))); process.exitCode = Number(process.env.PNPM_FIXTURE_EXIT ?? 0)\n')
  return { dir, wrapper }
}
