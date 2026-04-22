/// <reference path="../../../__typings__/index.d.ts" />
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { assertProject } from '@pnpm/assert-project'
import { importCommand } from '@pnpm/installing.commands'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { fixtures } from '@pnpm/test-fixtures'
import { filterProjectsBySelectorObjectsFromDir } from '@pnpm/workspace.projects-filter'
import { temporaryDirectory } from 'tempy'

const f = fixtures(import.meta.dirname)
const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}`
const TMP = temporaryDirectory()

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
  minimumReleaseAge: 0,
  networkConcurrency: 16,
  offline: false,
  preferWorkspacePackages: true,
  proxy: undefined,
  pnpmHomeDir: '',
  configByUri: {},
  registries: { default: REGISTRY },
  registry: REGISTRY,
  rootProjectManifestDir: '',
  storeDir: path.join(TMP, 'store'),
  strictSsl: false,
  userAgent: 'pnpm',
  userConfig: {},
  useRunningStoreServer: false,
  useStoreServer: false,
  virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
}

test('import from shared yarn.lock of monorepo', async () => {
  f.prepare('workspace-has-shared-yarn-lock')
  const { allProjects, allProjectsGraph, selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])
  await importCommand.handler({
    ...DEFAULT_OPTS,
    allProjects: allProjects as any, // eslint-disable-line @typescript-eslint/no-explicit-any
    allProjectsGraph,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
    lockfileDir: process.cwd(),
    dir: process.cwd(),
    resolutionMode: 'highest', // TODO: this should work with the default resolution mode (TODOv8)
  }, [])

  const project = assertProject(process.cwd())
  const lockfile = project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['is-positive@1.0.0'])
  expect(lockfile.packages).toHaveProperty(['is-negative@1.0.1'])

  // node_modules is not created
  project.hasNot('is-positive')
  project.hasNot('is-negative')
})

test('import from shared package-lock.json of monorepo', async () => {
  f.prepare('workspace-has-shared-package-lock-json')
  const { allProjects, allProjectsGraph, selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])
  await importCommand.handler({
    ...DEFAULT_OPTS,
    allProjects: allProjects as any, // eslint-disable-line @typescript-eslint/no-explicit-any
    allProjectsGraph,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
    lockfileDir: process.cwd(),
    dir: process.cwd(),
    resolutionMode: 'highest', // TODO: this should work with the default resolution mode (TODOv8)
  }, [])

  const project = assertProject(process.cwd())
  const lockfile = project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['is-positive@1.0.0'])
  expect(lockfile.packages).toHaveProperty(['is-negative@1.0.1'])

  // node_modules is not created
  project.hasNot('is-positive')
  project.hasNot('is-negative')
})

test('import from shared npm-shrinkwrap.json of monorepo', async () => {
  f.prepare('workspace-has-shared-npm-shrinkwrap-json')
  const { allProjects, allProjectsGraph, selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])
  await importCommand.handler({
    ...DEFAULT_OPTS,
    allProjects: allProjects as any, // eslint-disable-line @typescript-eslint/no-explicit-any
    allProjectsGraph,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
    lockfileDir: process.cwd(),
    dir: process.cwd(),
    resolutionMode: 'highest', // TODO: this should work with the default resolution mode (TODOv8)
  }, [])

  const project = assertProject(process.cwd())
  const lockfile = project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['is-positive@1.0.0'])
  expect(lockfile.packages).toHaveProperty(['is-negative@1.0.1'])

  // node_modules is not created
  project.hasNot('is-positive')
  project.hasNot('is-negative')
})
