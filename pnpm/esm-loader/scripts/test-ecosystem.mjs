import { execFileSync, spawnSync } from 'node:child_process'
import console from 'node:console'
import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { pathToFileURL } from 'node:url'

import { assertIsolated, findDependency, prepareRepository } from './repository-fixture.mjs'
import { auditEnvironment, readAudit } from './repository-scenario.mjs'

const runner = { '--node': 'node', '--mocha': 'mocha', '--jest': 'jest' }[process.argv[2]] ?? 'vitest'
const inputs = process.argv.slice(runner === 'vitest' ? 2 : 3)
const cwd = inputs[0] === '--cwd' ? inputs.splice(0, 2)[1] : '.'
const [repository, ...args] = inputs
if (!repository || args.length === 0) throw new Error('Usage: test-ecosystem.mjs [--node|--mocha|--jest] [--cwd <relative directory>] <checkout> <runner arguments...>')
if (!cwd || path.isAbsolute(cwd) || path.normalize(cwd).split(path.sep).includes('..')) throw new Error('The working directory must be inside the checkout')
const repo = fs.realpathSync(repository)
const env = { ...process.env, NODE_OPTIONS: '', NODE_PATH: '' }
const runnerRoot = runner === 'node' ? null : findDependency(path.join(repo, cwd), runner)
if (runner !== 'node' && !runnerRoot) throw new Error(`Cannot find ${runner} from ${path.join(repo, cwd)}`)
const runnerMain = runnerRoot ? readBin(runnerRoot, runner) : null
const baselineArgs = runner === 'node' ? args : [path.join(runnerRoot, runnerMain), ...args]
const baseline = run(baselineArgs, { cwd: path.join(repo, cwd), env })
console.log(baseline.stdout, baseline.stderr)
if (baseline.status !== 0) throw new Error('The normal repository baseline failed')
const prepared = await prepareRepository(repo, { fullRepository: true })
const { root, manifest } = prepared
const entry = path.join('repo', cwd, `run-${runner}.mjs`)
const runnerEntry = path.join(root, entry)
if (runner !== 'node') fs.writeFileSync(runnerEntry, `await import(new URL(${JSON.stringify('./' + runnerMain)}, import.meta.resolve('${runner}/package.json')))\n`)
const auditPath = path.join(root, 'cas-loads.jsonl')
const casArgs = runner === 'node' ? args : [runnerEntry, ...args]
const cas = run(['--import', pathToFileURL(path.join(root, 'loader.mjs')).href, ...casArgs], {
  cwd: path.join(root, 'repo', cwd), env: { ...env, ...auditEnvironment(root, auditPath), PNPM_LOADER_MANIFEST: path.join(root, '.pnpm-store.json') },
})
for (const [name, result] of Object.entries({ baseline, cas })) {
  fs.writeFileSync(path.join(root, name + '.stdout'), result.stdout)
  fs.writeFileSync(path.join(root, name + '.stderr'), result.stderr)
}
assertIsolated(root)
const report = {
  repo, revision: execFileSync('git', ['rev-parse', 'HEAD'], { cwd: repo, encoding: 'utf8' }).trim(),
  node: process.version, runner, cwd, entry, args, baseline: baseline.status, cas: cas.status, noNodeModules: true,
  casError: cas.error?.message, casSignal: cas.signal,
  packages: prepared.ids.size, storedPackages: Object.values(manifest.packages).filter(pkg => pkg.files).length,
  files: prepared.fileCount, bytes: prepared.bytes, ...readAudit(auditPath),
}
fs.writeFileSync(path.join(root, 'ecosystem-results.json'), JSON.stringify(report, null, 2))
console.log(JSON.stringify({ root, ...report }, null, 2), cas.stdout, cas.stderr)
process.exitCode = cas.status ?? 1

function readBin (directory, name) {
  const { bin } = JSON.parse(fs.readFileSync(path.join(directory, 'package.json'), 'utf8'))
  const entry = typeof bin === 'string' ? bin : bin?.[name]
  if (typeof entry !== 'string') throw new Error(`No ${name} binary declared in ${directory}`)
  return entry
}

function run (args, options) {
  const result = spawnSync(process.execPath, args, { ...options, encoding: 'utf8', timeout: 180000, maxBuffer: 16 * 1024 * 1024 })
  if (result.error && result.error.code !== 'ETIMEDOUT') throw result.error
  return result
}
