import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { STORE_VERSION } from '@pnpm/constants'
import { fetch, install } from '@pnpm/installing.commands'
import { prepare } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { closeAllStoreIndexes } from '@pnpm/store.index'
import { fixtures } from '@pnpm/test-fixtures'
import { finishWorkers } from '@pnpm/worker'
import { rimrafSync } from '@zkochan/rimraf'

const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}`

const DEFAULT_OPTIONS = {
  argv: {
    original: [],
  },
  bail: false,
  bin: 'node_modules/.bin',
  cliOptions: {},
  deployAllFiles: false,
  excludeLinksFromLockfile: false,
  extraEnv: {},
  include: {
    dependencies: true,
    devDependencies: true,
    optionalDependencies: true,
  },
  lock: true,
  preferWorkspacePackages: true,
  pnpmfile: ['.pnpmfile.cjs'],
  pnpmHomeDir: '',
  configByUri: {},
  registries: {
    default: REGISTRY_URL,
  },
  rootProjectManifestDir: '',
  sort: true,
  userConfig: {},
  workspaceConcurrency: 1,
  virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
}

test('fetch dependencies', async () => {
  const project = prepare({
    dependencies: { 'is-positive': '1.0.0' },
    devDependencies: { 'is-negative': '1.0.0' },
  })
  const storeDir = path.resolve('store')

  await install.handler({
    ...DEFAULT_OPTIONS,
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    linkWorkspacePackages: true,
    storeDir,
  })

  rimrafSync(path.resolve(project.dir(), 'node_modules'))
  rimrafSync(path.resolve(project.dir(), './package.json'))

  project.storeHasNot('is-negative')
  project.storeHasNot('is-positive')

  await fetch.handler({
    ...DEFAULT_OPTIONS,
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    storeDir,
  })

  project.storeHas('is-positive')
  project.storeHas('is-negative')
})

test('fetch production dependencies', async () => {
  const project = prepare({
    dependencies: { 'is-positive': '1.0.0' },
    devDependencies: { 'is-negative': '1.0.0' },
  })
  const storeDir = path.resolve('store')
  await install.handler({
    ...DEFAULT_OPTIONS,
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    linkWorkspacePackages: true,
    storeDir,
  })

  rimrafSync(path.resolve(project.dir(), 'node_modules'))
  rimrafSync(path.resolve(project.dir(), './package.json'))

  project.storeHasNot('is-negative')
  project.storeHasNot('is-positive')

  await fetch.handler({
    ...DEFAULT_OPTIONS,
    cacheDir: path.resolve('cache'),
    dev: true,
    dir: process.cwd(),
    storeDir,
  })

  project.storeHasNot('is-negative')
  project.storeHas('is-positive')
})

test('fetch only dev dependencies', async () => {
  const project = prepare({
    dependencies: { 'is-positive': '1.0.0' },
    devDependencies: { 'is-negative': '1.0.0' },
  })
  const storeDir = path.resolve('store')
  await install.handler({
    ...DEFAULT_OPTIONS,
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    linkWorkspacePackages: true,
    storeDir,
  })

  rimrafSync(path.resolve(project.dir(), 'node_modules'))
  rimrafSync(path.resolve(project.dir(), './package.json'))

  project.storeHasNot('is-negative')
  project.storeHasNot('is-positive')

  await fetch.handler({
    ...DEFAULT_OPTIONS,
    cacheDir: path.resolve('cache'),
    dev: true,
    dir: process.cwd(),
    storeDir,
  })

  project.storeHas('is-negative')
  project.storeHasNot('is-positive')
})

// Regression test for https://github.com/pnpm/pnpm/issues/10460
// pnpm fetch should skip local file: protocol dependencies
// because they won't be available in Docker builds
test('fetch skips file: protocol dependencies that do not exist', async () => {
  const project = prepare({
    dependencies: {
      'is-positive': '1.0.0',
      '@local/pkg': 'file:./local-pkg',
    },
  })
  const storeDir = path.resolve('store')
  const localPkgDir = path.resolve(project.dir(), 'local-pkg')

  // Create the local package for initial install to generate lockfile
  fs.mkdirSync(localPkgDir, { recursive: true })
  fs.writeFileSync(
    path.join(localPkgDir, 'package.json'),
    JSON.stringify({ name: '@local/pkg', version: '1.0.0' })
  )

  // Create a lockfile with the file: dependency
  await install.handler({
    ...DEFAULT_OPTIONS,
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    linkWorkspacePackages: true,
    storeDir,
  })

  rimrafSync(path.resolve(project.dir(), 'node_modules'))
  rimrafSync(path.resolve(project.dir(), './package.json'))
  // Remove the local package directory to simulate Docker build scenario
  rimrafSync(localPkgDir)

  project.storeHasNot('is-positive')

  // This should not throw an error even though the file: dependency doesn't exist
  await fetch.handler({
    ...DEFAULT_OPTIONS,
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    storeDir,
  })

  project.storeHas('is-positive')
})

test('fetch populates global virtual store links/', async () => {
  prepare({
    dependencies: {
      'is-positive': '1.0.0',
    },
    devDependencies: {
      'is-negative': '1.0.0',
    },
  })
  const storeDir = path.resolve('store')
  const globalVirtualStoreDir = path.join(storeDir, STORE_VERSION, 'links')

  // Generate the lockfile only — no need for a full install
  await install.handler({
    ...DEFAULT_OPTIONS,
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    linkWorkspacePackages: true,
    lockfileOnly: true,
    storeDir,
  })

  // Drain workers and close SQLite connections before removing the store (required on Windows)
  await finishWorkers()
  closeAllStoreIndexes()

  // Remove the store — simulate a cold start with only the lockfile
  rimrafSync(storeDir)

  // Fetch with enableGlobalVirtualStore — should populate links/
  await fetch.handler({
    ...DEFAULT_OPTIONS,
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    storeDir,
    enableGlobalVirtualStore: true,
  })

  // The global virtual store links/ directory should exist and contain packages
  expect(fs.existsSync(globalVirtualStoreDir)).toBeTruthy()
  const entries = fs.readdirSync(globalVirtualStoreDir)
  expect(entries.length).toBeGreaterThan(0)
})

test('fetch applies patches to dependencies when patchedDependencies key is bare package name', async () => {
  const f = fixtures(import.meta.dirname)
  const project = prepare({
    dependencies: { '@pnpm.e2e/console-log': '1.0.0' },
  })
  fs.mkdirSync('patches', { recursive: true })
  fs.copyFileSync(f.find('patchedDependencies/console-log-replace-1st-line.patch'), 'patches/console-log.patch')

  const patchedDependencies = { '@pnpm.e2e/console-log': 'patches/console-log.patch' }
  const cacheDir = path.resolve(project.dir(), 'cache')
  const storeDir = path.resolve(project.dir(), 'store')

  await install.handler({
    ...DEFAULT_OPTIONS,
    cacheDir,
    dir: project.dir(),
    linkWorkspacePackages: false,
    lockfileOnly: true,
    storeDir,
    patchedDependencies,
  })

  await fetch.handler({
    ...DEFAULT_OPTIONS,
    cacheDir,
    dir: project.dir(),
    storeDir,
    patchedDependencies,
  })

  const virtualStoreDir = path.resolve(project.dir(), 'node_modules', '.pnpm')
  const consoleLogDirs = fs.readdirSync(virtualStoreDir).filter(d => d.startsWith('@pnpm.e2e+console-log@'))
  expect(consoleLogDirs.length).toBeGreaterThan(0)

  const patchedIndexJsAfterFetch = fs.readFileSync(path.join(virtualStoreDir, consoleLogDirs[0], 'node_modules/@pnpm.e2e/console-log/index.js'), 'utf8')
  expect(patchedIndexJsAfterFetch).toContain('FIRST LINE')
})
