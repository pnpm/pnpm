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
    const expectedNames = Object.keys(expected)
    assertInstalledBins('node_modules/.bin', expectedNames)
    if (nodeLinker === 'isolated') {
      assertInstalledBins('packages/b/node_modules/.bin', expectedNames)
      assertInstalledBins('packages/c/node_modules/.bin', expectedNames)
    }
    assertBins('root', expected)
  }
  writeBins({ 'a-one': 'one.js', 'a-two': 'two.js' })
  install()
  assertInstalledBins('node_modules/.bin', ['a-one', 'a-two'])

  writeBins({ 'a-one': 'one.js' })
  install()
  assertInstalledBins('node_modules/.bin', ['a-one'])

  writeBins(null)
  install(['--force'])
  assertInstalledBins('node_modules/.bin', [])
})

test.each(['isolated', 'hoisted'])('hoisted workspace package prunes removed bins from root with %s linker', (nodeLinker) => {
  prepareEmpty()
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['packages/*'],
    nodeLinker,
    hoistWorkspacePackages: true,
    publicHoistPattern: ['*'],
    verifyDepsBeforeRun: false,
  })
  fs.writeFileSync('package.json', JSON.stringify({ name: 'root', private: true }))
  fs.mkdirSync('packages/b', { recursive: true })
  fs.writeFileSync('packages/b/package.json', JSON.stringify({ name: 'b', private: true, dependencies: { a: 'workspace:*' } }))
  fs.mkdirSync('packages/a', { recursive: true })
  fs.writeFileSync('packages/a/one.js', "#!/usr/bin/env node\nconsole.log('one')\n")
  fs.writeFileSync('packages/a/two.js', "#!/usr/bin/env node\nconsole.log('two')\n")

  writeBins({ 'a-one': 'one.js', 'a-two': 'two.js' })
  install()
  assertInstalledBins('node_modules/.bin', ['a-one', 'a-two'])

  writeBins({ 'a-renamed': 'two.js' })
  install(['--frozen-lockfile'])
  assertInstalledBins('node_modules/.bin', ['a-renamed'])

  writeBins(null)
  install()
  assertInstalledBins('node_modules/.bin', [])
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

function assertInstalledBins (binsDir: string, expected: string[]): void {
  if (!fs.existsSync(binsDir)) {
    expect(expected).toEqual([])
    return
  }
  const isWindows = process.platform === 'win32'
  const actual = Array.from(new Set(
    fs.readdirSync(binsDir).map((entry) => {
      if (!isWindows) return entry
      const lower = entry.toLowerCase()
      for (const suffix of ['.cmd', '.ps1', '.exe']) {
        if (lower.endsWith(suffix)) return lower.slice(0, -suffix.length)
      }
      return lower
    })
  )).sort()
  const normalizedExpected = expected
    .map((name) => isWindows ? name.toLowerCase() : name)
    .sort()
  expect(actual).toEqual(normalizedExpected)
}
