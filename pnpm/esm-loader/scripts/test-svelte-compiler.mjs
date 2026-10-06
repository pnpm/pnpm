import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import console from 'node:console'
import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath, pathToFileURL, URL } from 'node:url'

import { assertIsolated } from './repository-fixture.mjs'
import { readAudit } from './repository-scenario.mjs'

if (!process.argv[2]) throw new Error('Pass the retained Svelte fixture path from test-ecosystem.mjs')
const root = path.resolve(process.argv[2])
const { repo, revision } = JSON.parse(fs.readFileSync(path.join(root, 'ecosystem-results.json'), 'utf8'))
const entry = fileURLToPath(new URL('./compile-svelte-corpus.mjs', import.meta.url))
const env = { ...process.env, NODE_OPTIONS: '', NODE_PATH: '' }
const baseline = run([entry, repo], env)
const auditPath = path.join(root, 'compiler-cas-loads.jsonl')
fs.writeFileSync(auditPath, '')
const cas = run(['--import', pathToFileURL(path.join(root, 'loader.mjs')).href, entry, path.join(root, 'repo')], {
  ...env, PNPM_LOADER_MANIFEST: path.join(root, '.store-manifest.json'), PNPM_LOADER_AUDIT: auditPath,
})
assert.deepEqual(cas, baseline)
assertIsolated(root)
const report = {
  revision, node: process.version, compiled: cas.length, outputsMatch: true, materializedPackages: 0,
  ...readAudit(auditPath),
}
assert.ok(report.casPackages > 0)
fs.writeFileSync(path.join(root, 'compiler-results.json'), JSON.stringify(report, null, 2))
console.log(report)

function run (args, env) {
  const result = spawnSync(process.execPath, args, { env, encoding: 'utf8', timeout: 120000, maxBuffer: 16 * 1024 * 1024 })
  if (result.error) throw result.error
  assert.equal(result.status, 0, result.stderr)
  return JSON.parse(result.stdout)
}
