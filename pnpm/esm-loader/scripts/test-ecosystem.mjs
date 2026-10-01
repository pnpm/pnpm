import { execFileSync, spawnSync } from 'node:child_process'
import console from 'node:console'
import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { pathToFileURL } from 'node:url'

import { assertIsolated, prepareRepository } from './repository-fixture.mjs'
import { auditEnvironment, readAudit } from './repository-scenario.mjs'

const runner = process.argv[2] === '--node' ? 'node' : 'vitest'
const [repository, ...args] = process.argv.slice(runner === 'node' ? 3 : 2)
if (!repository || args.length === 0) throw new Error('Usage: test-ecosystem.mjs [--node] <checkout> <runner arguments...>')
const repo = fs.realpathSync(repository)
const env = { ...process.env, NODE_OPTIONS: '', NODE_PATH: '' }
const baselineArgs = runner === 'node' ? args : [path.join(repo, 'node_modules/vitest/vitest.mjs'), ...args]
const baseline = run(baselineArgs, { cwd: repo, env })
console.log(baseline.stdout, baseline.stderr)
if (baseline.status !== 0) throw new Error('The normal repository baseline failed')
const prepared = await prepareRepository(repo, { fullRepository: true })
const { root, manifest } = prepared
fs.writeFileSync(path.join(root, 'repo/run-vitest.mjs'), "await import(new URL('./vitest.mjs', import.meta.resolve('vitest/package.json')))\n")
const auditPath = path.join(root, 'cas-loads.jsonl')
const casArgs = runner === 'node' ? args : [path.join(root, 'repo/run-vitest.mjs'), ...args]
const cas = run(['--import', pathToFileURL(path.join(root, 'loader.mjs')).href, ...casArgs], {
  cwd: path.join(root, 'repo'), env: { ...env, ...auditEnvironment(root, auditPath), PNPM_LOADER_MANIFEST: path.join(root, '.pnpm-store.json') },
})
for (const [name, result] of Object.entries({ baseline, cas })) {
  fs.writeFileSync(path.join(root, name + '.stdout'), result.stdout)
  fs.writeFileSync(path.join(root, name + '.stderr'), result.stderr)
}
assertIsolated(root)
const report = {
  repo, revision: execFileSync('git', ['rev-parse', 'HEAD'], { cwd: repo, encoding: 'utf8' }).trim(),
  node: process.version, runner, args, baseline: baseline.status, cas: cas.status, noNodeModules: true,
  packages: prepared.ids.size, storedPackages: Object.values(manifest.packages).filter(pkg => pkg.files).length,
  files: prepared.fileCount, bytes: prepared.bytes, ...readAudit(auditPath),
}
fs.writeFileSync(path.join(root, 'ecosystem-results.json'), JSON.stringify(report, null, 2))
console.log(JSON.stringify({ root, ...report }, null, 2), cas.stdout, cas.stderr)
process.exitCode = cas.status ?? 1

function run (args, options) {
  const result = spawnSync(process.execPath, args, { ...options, encoding: 'utf8', timeout: 180000, maxBuffer: 16 * 1024 * 1024 })
  if (result.error) throw result.error
  return result
}
