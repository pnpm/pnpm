import path from 'path'
import { install, fetch } from '@pnpm/plugin-commands-installation'
import { prepare } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import rimraf from '@zkochan/rimraf'

const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}`

const DEFAULT_OPTIONS = {
  argv: {
    original: [],
  },
  bail: false,
  bin: 'node_modules/.bin',
  cliOptions: {},
  deployAllFiles: false,
  extraEnv: {},
  include: {
    dependencies: true,
    devDependencies: true,
    optionalDependencies: true,
  },
  lock: true,
  pnpmfile: '.pnpmfile.cjs',
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

  await rimraf(path.resolve(project.dir(), 'node_modules'))
  await rimraf(path.resolve(project.dir(), './package.json'))

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

  await rimraf(path.resolve(project.dir(), 'node_modules'))
  await rimraf(path.resolve(project.dir(), './package.json'))

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

  await rimraf(path.resolve(project.dir(), 'node_modules'))
  await rimraf(path.resolve(project.dir(), './package.json'))

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
