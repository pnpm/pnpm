import { spawnSync } from 'node:child_process'
import console from 'node:console'
import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath, URL } from 'node:url'

import { assertIsolated, prepareRepository } from './repository-fixture.mjs'

const repo = path.resolve(fileURLToPath(new URL('../../..', import.meta.url)))
const suite = 'pnpm11/cli/parse-cli-args'
const baseline = run(process.execPath, [path.join(repo, 'node_modules/jest/bin/jest.js'), '--runInBand', '--coverage=false', '--runTestsByPath', 'test/index.ts'], {
  cwd: path.join(repo, suite), env: { ...process.env, NODE_OPTIONS: '--experimental-vm-modules --disable-warning=ExperimentalWarning' },
})
if (baseline.status !== 0) throw new Error(`Baseline failed:\n${baseline.stderr}`)
console.log(baseline.stderr)
const prepared = await prepareRepository(repo)
const { root } = prepared
console.log(`Prepared ${prepared.ids.size} packages and ${prepared.fileCount} files (${Math.round(prepared.bytes / 1024 / 1024)} MiB) in ${root}`)
const env = {
  ...process.env, NODE_PATH: '', NODE_OPTIONS: '', XDG_CONFIG_HOME: path.join(root, 'config'),
  PNPM_LOADER_MANIFEST: path.join(root, '.pnpm-store.json'), UNRS_RESOLVER_NODE_RESOLUTION: '1',
}
const cases = [
  { name: 'parse-cli-args', directory: suite, entry: 'repo/run-jest.cjs', args: ['--runInBand', '--coverage=false', '--runTestsByPath', 'test/index.ts'] },
  { name: 'pnpm-cli-tests', directory: 'pnpm11/pnpm', entry: 'repo/run-jest.cjs', args: ['--runInBand', '--coverage=false', '--runTestsByPath', 'test/withCommand.test.ts'] },
  { name: 'unbundled-version', directory: 'pnpm11/pnpm', entry: 'repo/pnpm11/pnpm/lib/pnpm.js', args: ['with', 'current', '--version'] },
  { name: 'unbundled-help', directory: 'pnpm11/pnpm', entry: 'repo/pnpm11/pnpm/lib/pnpm.js', args: ['with', 'current', 'help'] },
]
const results = []
for (const testCase of cases) {
  const args = ['--import', path.join(root, 'loader.mjs'), '--experimental-vm-modules', path.join(root, testCase.entry), ...testCase.args]
  const result = run(process.execPath, args, { cwd: path.join(root, 'repo', testCase.directory), env })
  fs.writeFileSync(path.join(root, testCase.name + '.stdout'), result.stdout)
  fs.writeFileSync(path.join(root, testCase.name + '.stderr'), result.stderr)
  results.push({ name: testCase.name, status: result.status, args })
  console.log(`${testCase.name}: exit ${result.status}\n${result.stderr}`)
}
diagnoseJestResolver(prepared, env)
assertIsolated(root)
fs.writeFileSync(path.join(root, 'results.json'), JSON.stringify({ baselinePassed: true, noNodeModules: true, packages: prepared.ids.size, files: prepared.fileCount, results }, null, 2))
console.log(`Results and retained fixture: ${root}`)
process.exitCode = results.some(result => result.status !== 0) ? 1 : 0

function run (command, args, options) {
  const result = spawnSync(command, args, { ...options, encoding: 'utf8', timeout: 120000, maxBuffer: 16 * 1024 * 1024 })
  if (result.error) throw result.error
  return result
}

function diagnoseJestResolver ({ root, manifest }, env) {
  const diagnostic = JSON.parse(JSON.stringify(manifest))
  const jestId = diagnostic.packages['.'].dependencies.jest
  const cliId = diagnostic.packages[jestId].dependencies['jest-cli']
  const configId = diagnostic.packages[cliId].dependencies['jest-config']
  diagnostic.packages['.'].dependencies['jest-resolve'] = diagnostic.packages[configId].dependencies['jest-resolve']
  const manifestPath = path.join(root, '.pnpm-diagnostic.json')
  const entry = path.join(root, 'repo/diagnose.cjs')
  fs.writeFileSync(manifestPath, JSON.stringify(diagnostic))
  fs.writeFileSync(entry, "const Resolver = require('jest-resolve').default; Resolver.findNodeModule('@pnpm/jest-config/jest-preset.js', { basedir: process.cwd(), throwIfNotFound: true });\n")
  const result = run(process.execPath, ['--import', path.join(root, 'loader.mjs'), entry], {
    cwd: path.join(root, 'repo', suite), env: { ...env, PNPM_LOADER_MANIFEST: manifestPath },
  })
  fs.writeFileSync(path.join(root, 'jest-resolver.stderr'), result.stderr)
  console.log(`Resolver diagnostics: ${path.join(root, 'jest-resolver.stderr')}`)
}
