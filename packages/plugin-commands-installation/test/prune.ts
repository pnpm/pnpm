import path from 'path'
import { add, install, link, prune } from '@pnpm/plugin-commands-installation'
import prepare from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import fixtures from '@pnpm/test-fixtures'

const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}`
const f = fixtures(__dirname)

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
  linkWorkspacePackages: true,
  pnpmfile: '.pnpmfile.cjs',
  pnpmHomeDir: '',
  rawConfig: { registry: REGISTRY_URL },
  rawLocalConfig: { registry: REGISTRY_URL },
  registries: {
    default: REGISTRY_URL,
  },
  sort: true,
  userConfig: {},
  workspaceConcurrency: 1,
}

test('prune removes external link that is not in package.json', async () => {
  const project = prepare(undefined)
  const storeDir = path.resolve('store')
  f.copy('local-pkg', 'local')

  await link.handler({
    ...DEFAULT_OPTIONS,
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    storeDir,
  }, ['./local'])

  await project.has('local-pkg')

  await prune.handler({
    ...DEFAULT_OPTIONS,
    cacheDir: path.resolve('cache'),
    dir: process.cwd(),
    storeDir,
  })

  await project.hasNot('local-pkg')
})

test('prune keeps hoisted dependencies', async () => {
  const project = prepare(undefined)
  const storeDir = path.resolve('store')
  const cacheDir = path.resolve('cache')

  await add.handler({
    ...DEFAULT_OPTIONS,
    cacheDir,
    dir: process.cwd(),
    storeDir,
  }, ['pkg-with-1-dep@100.0.0'])

  await prune.handler({
    ...DEFAULT_OPTIONS,
    cacheDir,
    dir: process.cwd(),
    storeDir,
  })

  await project.hasNot('dep-of-pkg-with-1-dep')
})

test('prune removes dev dependencies', async () => {
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

  await prune.handler({
    ...DEFAULT_OPTIONS,
    cacheDir: path.resolve('cache'),
    dev: false,
    dir: process.cwd(),
    storeDir,
  })

  await project.has('is-positive')
  await project.has('.pnpm/is-positive@1.0.0')
  await project.hasNot('is-negative')
  await project.hasNot('.pnpm/is-negative@1.0.0')
})
