import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepareEmpty } from '@pnpm/prepare'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpmSync } from '../utils/index.js'

test.each(['isolated', 'hoisted'])('repeat install relinks changed workspace bins with %s linker', (nodeLinker) => {
  createWorkspace(nodeLinker)
  const declarations: Array<[null | string | Record<string, string>, Record<string, string>]> = [
    [null, {}],
    [{ 'a-one': 'one.js' }, { 'a-one': 'one' }],
    [{ 'a-one': 'one.js', 'a-two': 'two.js' }, { 'a-one': 'one', 'a-two': 'two' }],
    [{ 'a-one': 'two.js', 'a-renamed': 'one.js' }, { 'a-one': 'two', 'a-renamed': 'one' }],
    ['two.js', { a: 'two' }],
    [null, {}],
  ]
  for (const [bin, expected] of declarations) {
    writeBins(bin)
    const lockfile = fs.existsSync('pnpm-lock.yaml') ? fs.readFileSync('pnpm-lock.yaml', 'utf8') : undefined
    install(lockfile == null ? [] : ['--frozen-lockfile'])
    if (lockfile != null) expect(fs.readFileSync('pnpm-lock.yaml', 'utf8')).toBe(lockfile)
    assertBins('root', expected)
  }
  install()
})

test.each(['isolated', 'hoisted'])('filtered installs keep unselected workspace bins stale with %s linker', (nodeLinker) => {
  createWorkspace(nodeLinker)
  writeBins({ 'a-one': 'one.js' })
  install()
  writeBins({ 'a-one': 'one.js', 'a-two': 'two.js' })
  for (const consumer of ['b', 'c', 'root']) {
    install(['--filter', consumer, '--frozen-lockfile'])
    assertBins(consumer, { 'a-two': 'two' })
  }
  install(['--frozen-lockfile'])
  const statePath = 'node_modules/.pnpm-workspace-state-v1.json'
  const state = JSON.parse(fs.readFileSync(statePath, 'utf8'))
  delete state.validation.workspaceBins
  fs.writeFileSync(statePath, JSON.stringify(state))
  fs.rmSync('node_modules/.bin', { recursive: true })
  install(['--frozen-lockfile'])
  assertBins('root', { 'a-two': 'two' })
})

function createWorkspace (nodeLinker: string): void {
  prepareEmpty()
  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['packages/*'], nodeLinker, verifyDepsBeforeRun: false })
  for (const [name, dir] of [['root', '.'], ['b', 'packages/b'], ['c', 'packages/c']]) {
    fs.mkdirSync(dir, { recursive: true })
    fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({ name, private: true, dependencies: { a: 'workspace:*' } }))
  }
  fs.mkdirSync('packages/a', { recursive: true })
  fs.writeFileSync('packages/a/one.js', "#!/usr/bin/env node\nconsole.log('one')\n")
  fs.writeFileSync('packages/a/two.js', "#!/usr/bin/env node\nconsole.log('two')\n")
}

function writeBins (bin: null | string | Record<string, string>): void {
  fs.writeFileSync('packages/a/package.json', JSON.stringify({ name: 'a', version: '1.0.0', bin }))
}

function install (args: string[] = []): void {
  const result = execPnpmSync(['install', '--offline', '--ignore-scripts', ...args])
  expect({ status: result.status, stderr: result.stderr.toString() }).toMatchObject({ status: 0 })
}

function assertBins (consumer: string, expected: Record<string, string>): void {
  for (const [name, output] of Object.entries(expected)) {
    const result = execPnpmSync(['--filter', consumer, '--config.verify-deps-before-run=error', 'exec', name])
    expect({ name, status: result.status, stderr: result.stderr.toString() }).toMatchObject({ status: 0 })
    expect(result.stdout.toString().trim()).toBe(output)
  }
}
