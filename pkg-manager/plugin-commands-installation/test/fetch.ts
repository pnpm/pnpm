import fs from 'fs'
import path from 'path'
import { install, fetch } from '@pnpm/plugin-commands-installation'
import { prepare } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { sync as rimraf } from '@zkochan/rimraf'

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
  rawConfig: { registry: REGISTRY_URL },
  rawLocalConfig: { registry: REGISTRY_URL },
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

  rimraf(path.resolve(project.dir(), 'node_modules'))
  rimraf(path.resolve(project.dir(), './package.json'))

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

  rimraf(path.resolve(project.dir(), 'node_modules'))
  rimraf(path.resolve(project.dir(), './package.json'))

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

  rimraf(path.resolve(project.dir(), 'node_modules'))
  rimraf(path.resolve(project.dir(), './package.json'))

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

  rimraf(path.resolve(project.dir(), 'node_modules'))
  rimraf(path.resolve(project.dir(), './package.json'))
  // Remove the local package directory to simulate Docker build scenario
  rimraf(localPkgDir)

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
