import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { URL } from 'node:url'

const register = new URL('../register.mjs', import.meta.url).href

export function fixture (context) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-store-loader-'))
  context.after(() => fs.rmSync(root, { recursive: true, force: true }))
  const manifest = { version: 1, storeDir: './store', packages: { '.': { root: '.', dependencies: {} } } }
  function write (filename, content) {
    const target = path.join(root, filename)
    fs.mkdirSync(path.dirname(target), { recursive: true })
    fs.writeFileSync(target, content)
  }
  function add (id, contents, dependencies = {}) {
    const files = {}
    for (const [name, source] of Object.entries(contents)) {
      const hash = createHash('sha512').update(source).digest('hex')
      files[name] = hash
      write(`store/files/${hash.slice(0, 2)}/${hash.slice(2)}`, source)
    }
    manifest.packages[id] = { files, dependencies }
    return files
  }
  function run (source, options = {}) {
    write('.store-manifest.json', JSON.stringify(manifest))
    write('app.mjs', source)
    const result = spawnSync(process.execPath, ['--import', register, ...(options.args ?? []), 'app.mjs'], {
      cwd: root, encoding: 'utf8', env: { ...process.env, ...options.env, PNPM_LOADER_MANIFEST: path.join(root, '.store-manifest.json') },
    })
    assert.equal(fs.existsSync(path.join(root, 'node_modules')), false)
    assert.equal(fs.existsSync(path.join(root, '.pnpm-loader')), false)
    if (!options.failure) assert.equal(result.status, 0, result.stderr)
    else assert.notEqual(result.status, 0, result.stdout)
    return result
  }
  return { root, manifest, write, add, run }
}
