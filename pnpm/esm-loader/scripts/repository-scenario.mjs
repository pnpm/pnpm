import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { pathToFileURL } from 'node:url'

export function runSuite (root, manifestPath, scenario) {
  const result = spawnSync(process.execPath, [
    '--import', pathToFileURL(path.join(root, 'loader.mjs')).href, '--experimental-vm-modules',
    path.join(root, scenario.entry), ...scenario.args,
  ], {
    cwd: path.join(root, scenario.cwd), encoding: 'utf8', timeout: 120000,
    env: { ...process.env, NODE_PATH: '', NODE_OPTIONS: '--import=' + pathToFileURL(path.join(root, 'loader.mjs')).href, PNPM_LOADER_MANIFEST: manifestPath, UNRS_RESOLVER_NODE_RESOLUTION: '1' },
  })
  if (result.error) throw result.error
  return result
}

export function readScenario (root) {
  const report = path.join(root, 'ecosystem-results.json')
  if (fs.existsSync(report)) {
    return { entry: 'repo/run-vitest.mjs', cwd: 'repo', args: JSON.parse(fs.readFileSync(report, 'utf8')).args }
  }
  return {
    entry: 'repo/run-jest.cjs', cwd: 'repo/pnpm11/cli/parse-cli-args',
    args: ['--runInBand', '--no-cache', '--coverage=false', '--runTestsByPath', 'test/index.ts'],
  }
}

