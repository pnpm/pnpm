import assert from 'node:assert/strict'
import { execFileSync, spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'

const loader = fileURLToPath(new URL('../', import.meta.url))

test('the committed loader matches the runtime sources', () => {
  execFileSync(process.execPath, [path.join(loader, 'scripts/bundle-runtime.mjs'), '--check'], { stdio: 'inherit' })
})

test('check rejects a stale bundle without rewriting it', () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-loader-check-'))
  try {
    const isolatedLoader = path.join(directory, 'esm-loader')
    fs.cpSync(loader, isolatedLoader, {
      recursive: true,
      filter: source => !['node_modules', 'test'].includes(path.basename(source)),
    })
    fs.symlinkSync(fs.realpathSync(path.join(loader, 'node_modules')), path.join(isolatedLoader, 'node_modules'), 'junction')
    const output = path.join(directory, 'crates/deps-restorer/src/cas-loader.mjs.inc')
    fs.mkdirSync(path.dirname(output), { recursive: true })
    const stale = '// deliberately stale runtime\n'
    fs.writeFileSync(output, stale)
    const result = spawnSync(process.execPath, [path.join(isolatedLoader, 'scripts/bundle-runtime.mjs'), '--check'], { encoding: 'utf8' })
    assert.ifError(result.error)
    assert.equal(result.status, 1, result.stderr)
    assert.match(result.stderr, /The embedded loader is stale/)
    assert.equal(fs.readFileSync(output, 'utf8'), stale)
  } finally {
    fs.rmSync(directory, { recursive: true, force: true })
  }
})
