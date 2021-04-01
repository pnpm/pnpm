import path from 'path'
import { install, fetch } from '@pnpm/plugin-commands-installation'
import prepare from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import rimraf from 'rimraf'
import { promisify } from 'util'

const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}`

const DEFAULT_OPTIONS = {
  argv: {
    original: [],
  },
  bail: false,
  bin: 'node_modules/.bin',
  cliOptions: {},
  include: {
    dependencies: true,
    devDependencies: true,
    optionalDependencies: true,
  },
  lock: true,
  pnpmfile: '.pnpmfile.cjs',
  rawConfig: { registry: REGISTRY_URL },
  rawLocalConfig: { registry: REGISTRY_URL },
  registries: {
    default: REGISTRY_URL,
  },
  sort: true,
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
    dir: process.cwd(),
    linkWorkspacePackages: true,
    storeDir,
  })

  await promisify(rimraf)(path.resolve(project.dir(), 'node_modules'))
  await promisify(rimraf)(path.resolve(project.dir(), './package.json'))

  await project.storeHasNot('is-negative')
  await project.storeHasNot('is-positive')

  await fetch.handler({
    ...DEFAULT_OPTIONS,
    dir: process.cwd(),
    storeDir,
  })

  await project.storeHas('is-positive')
  await project.storeHas('is-negative')
})

test('fetch production dependencies', async () => {
  const project = prepare({
    dependencies: { 'is-positive': '1.0.0' },
    devDependencies: { 'is-negative': '1.0.0' },
  })
  const storeDir = path.resolve('store')
  await install.handler({
    ...DEFAULT_OPTIONS,
    dir: process.cwd(),
    linkWorkspacePackages: true,
    storeDir,
  })

  await promisify(rimraf)(path.resolve(project.dir(), 'node_modules'))
  await promisify(rimraf)(path.resolve(project.dir(), './package.json'))

  await project.storeHasNot('is-negative')
  await project.storeHasNot('is-positive')

  await fetch.handler({
    ...DEFAULT_OPTIONS,
    dev: true,
    dir: process.cwd(),
    storeDir,
  })

  await project.storeHasNot('is-negative')
  await project.storeHas('is-positive')
})

test('fetch only dev dependencies', async () => {
  const project = prepare({
    dependencies: { 'is-positive': '1.0.0' },
    devDependencies: { 'is-negative': '1.0.0' },
  })
  const storeDir = path.resolve('store')
  await install.handler({
    ...DEFAULT_OPTIONS,
    dir: process.cwd(),
    linkWorkspacePackages: true,
    storeDir,
  })

  await promisify(rimraf)(path.resolve(project.dir(), 'node_modules'))
  await promisify(rimraf)(path.resolve(project.dir(), './package.json'))

  await project.storeHasNot('is-negative')
  await project.storeHasNot('is-positive')

  await fetch.handler({
    ...DEFAULT_OPTIONS,
    dev: true,
    dir: process.cwd(),
    storeDir,
  })

  await project.storeHas('is-negative')
  await project.storeHasNot('is-positive')
})
