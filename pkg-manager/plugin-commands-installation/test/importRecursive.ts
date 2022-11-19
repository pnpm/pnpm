/// <reference path="../../../typings/index.d.ts" />
import path from 'path'
import { assertProject } from '@pnpm/assert-project'
import { importCommand } from '@pnpm/plugin-commands-installation'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { readProjects } from '@pnpm/filter-workspace-packages'
import { fixtures } from '@pnpm/test-fixtures'
import tempy from 'tempy'

const f = fixtures(__dirname)
const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}`
const TMP = tempy.directory()

const DEFAULT_OPTS = {
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
  pnpmHomeDir: '',
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
  f.prepare('workspace-has-shared-yarn-lock')
  const { allProjects, allProjectsGraph, selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await importCommand.handler({
    ...DEFAULT_OPTS,
    allProjects: allProjects as any, // eslint-disable-line @typescript-eslint/no-explicit-any
    allProjectsGraph,
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
  f.prepare('workspace-has-shared-package-lock-json')
  const { allProjects, allProjectsGraph, selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await importCommand.handler({
    ...DEFAULT_OPTS,
    allProjects: allProjects as any, // eslint-disable-line @typescript-eslint/no-explicit-any
    allProjectsGraph,
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
  f.prepare('workspace-has-shared-npm-shrinkwrap-json')
  const { allProjects, allProjectsGraph, selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await importCommand.handler({
    ...DEFAULT_OPTS,
    allProjects: allProjects as any, // eslint-disable-line @typescript-eslint/no-explicit-any
    allProjectsGraph,
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
