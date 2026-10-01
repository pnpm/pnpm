import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { pathToFileURL } from 'node:url'

export function runSuite (root, manifestPath, scenario) {
  const auditPath = path.join(root, 'suite-cas-loads.jsonl')
  const result = spawnSync(process.execPath, [
    '--import', pathToFileURL(path.join(root, 'loader.mjs')).href, '--experimental-vm-modules',
    ...(scenario.nodeArgs ?? [path.join(root, scenario.entry), ...scenario.args]),
  ], {
    cwd: path.join(root, scenario.cwd), encoding: 'utf8', timeout: 120000,
    env: { ...process.env, NODE_PATH: '', ...auditEnvironment(root, auditPath), PNPM_LOADER_MANIFEST: manifestPath, UNRS_RESOLVER_NODE_RESOLUTION: '1' },
  })
  if (result.error && result.error.code !== 'ETIMEDOUT') throw result.error
  return { ...result, ...readAudit(auditPath) }
}

export function readScenario (root) {
  const report = path.join(root, 'ecosystem-results.json')
  if (fs.existsSync(report)) {
    const { runner = 'vitest', cwd = '.', args } = JSON.parse(fs.readFileSync(report, 'utf8'))
    const scenario = { cwd: path.join('repo', cwd) }
    return runner === 'node' ? { ...scenario, nodeArgs: args } : { ...scenario, entry: `repo/run-${runner}.mjs`, args }
  }
  return {
    entry: 'repo/run-jest.cjs', cwd: 'repo/pnpm11/cli/parse-cli-args',
    args: ['--runInBand', '--no-cache', '--coverage=false', '--runTestsByPath', 'test/index.ts'],
  }
}

export function auditEnvironment (root, filename) {
  fs.writeFileSync(filename, '')
  const preloads = ['loader.mjs', 'audit-cas.mjs'].map(name => '--import=' + pathToFileURL(path.join(root, name)).href)
  return { NODE_OPTIONS: preloads.join(' '), PNPM_LOADER_AUDIT: filename }
}

export function readAudit (filename) {
  const lines = fs.readFileSync(filename, 'utf8').trim()
  const loaded = new Set(lines ? lines.split('\n').flatMap(line => JSON.parse(line)) : [])
  return {
    casModules: loaded.size,
    casPackages: new Set([...loaded].map(url => url.split('/.pnpm-loader/')[1].split('/')[0])).size,
  }
}
