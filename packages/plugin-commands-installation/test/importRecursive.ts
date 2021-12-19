/// <reference path="../../../typings/index.d.ts" />
import { promisify } from 'util'
import path from 'path'
import assertProject from '@pnpm/assert-project'
import { importCommand } from '@pnpm/plugin-commands-installation'
import { tempDir } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { readProjects } from '@pnpm/filter-workspace-packages'
import tempy from 'tempy'
import ncpCB from 'ncp'

const ncp = promisify(ncpCB)

const fixtures = path.join(__dirname, '../../../fixtures')

const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}`
const TMP = tempy.directory()

const DEFAULT_OPTS = {
  alwaysAuth: false,
  ca: undefined,
  cacheDir: path.join(TMP, 'cache'),
  cert: undefined,
  fetchRetries: 2,
  fetchRetryFactor: 90,
  fetchRetryMaxtimeout: 90,
  fetchRetryMintimeout: 10,
  httpsProxy: undefined,
  key: undefined,
  localAddress: undefined,
  lock: false,
  lockStaleDuration: 90,
  networkConcurrency: 16,
  offline: false,
  proxy: undefined,
  rawConfig: { registry: REGISTRY },
  registries: { default: REGISTRY },
  registry: REGISTRY,
  storeDir: path.join(TMP, 'store'),
  strictSsl: false,
  userAgent: 'pnpm',
  userConfig: {},
  useRunningStoreServer: false,
  useStoreServer: false,
}

test('import from shared yarn.lock of monorepo', async () => {
  tempDir()

  await ncp(path.join(fixtures, 'workspace-has-shared-yarn-lock'), process.cwd())
  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await importCommand.handler({
    ...DEFAULT_OPTS,
    allProjects,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
    lockfileDir: process.cwd(),
    dir: process.cwd(),
  }, [])

  const project = assertProject(process.cwd())
  const lockfile = await project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['/is-positive/1.0.0'])
  expect(lockfile.packages).toHaveProperty(['/is-negative/1.0.1'])

  // node_modules is not created
  await project.hasNot('is-positive')
  await project.hasNot('is-negative')
})

test('import from shared package-lock.json of monorepo', async () => {
  tempDir()

  await ncp(path.join(fixtures, 'workspace-has-shared-package-lock-json'), process.cwd())
  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await importCommand.handler({
    ...DEFAULT_OPTS,
    allProjects,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
    lockfileDir: process.cwd(),
    dir: process.cwd(),
  }, [])

  const project = assertProject(process.cwd())
  const lockfile = await project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['/is-positive/1.0.0'])
  expect(lockfile.packages).toHaveProperty(['/is-negative/1.0.1'])

  // node_modules is not created
  await project.hasNot('is-positive')
  await project.hasNot('is-negative')
})

test('import from shared npm-shrinkwrap.json of monorepo', async () => {
  tempDir()

  await ncp(path.join(fixtures, 'workspace-has-shared-npm-shrinkwrap-json'), process.cwd())
  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await importCommand.handler({
    ...DEFAULT_OPTS,
    allProjects,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
    lockfileDir: process.cwd(),
    dir: process.cwd(),
  }, [])

  const project = assertProject(process.cwd())
  const lockfile = await project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['/is-positive/1.0.0'])
  expect(lockfile.packages).toHaveProperty(['/is-negative/1.0.1'])

  // node_modules is not created
  await project.hasNot('is-positive')
  await project.hasNot('is-negative')
})
