import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { install } from '@pnpm/installing.commands'
import { preparePackages } from '@pnpm/prepare'
import { filterProjectsBySelectorObjectsFromDir } from '@pnpm/workspace.projects-filter'

import { DEFAULT_OPTS } from './utils/index.js'

const { deploy } = await import('@pnpm/releasing.commands')

test.each([
  { forceLegacyDeploy: false, virtualStoreDir: '.pnpm' },
  { forceLegacyDeploy: false, virtualStoreDir: 'node_modules/.custom' },
  { forceLegacyDeploy: true, virtualStoreDir: '.pnpm' },
])('deploy puts the virtual store at virtualStoreDir resolved against the deploy directory (%o)', async ({ forceLegacyDeploy, virtualStoreDir }) => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: {
        'is-positive': '1.0.0',
      },
    },
  ])

  const { allProjects, allProjectsGraph, selectedProjectsGraph } =
    await filterProjectsBySelectorObjectsFromDir(process.cwd(), [{ namePattern: 'project-1' }])
  const opts = {
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    lockfileDir: process.cwd(),
    workspaceDir: process.cwd(),
    virtualStoreDir,
  }

  await install.handler({ ...opts, allProjectsGraph, selectedProjectsGraph: allProjectsGraph })
  await deploy.handler({
    ...opts,
    forceLegacyDeploy,
    selectedProjectsGraph,
    sharedWorkspaceLockfile: true,
  }, ['deploy'])

  expect(fs.realpathSync('deploy/node_modules/is-positive')).toBe(
    fs.realpathSync(path.join('deploy', virtualStoreDir, 'is-positive@1.0.0/node_modules/is-positive'))
  )
  expect(fs.existsSync('deploy/node_modules/.pnpm/is-positive@1.0.0')).toBe(false)
  if (!forceLegacyDeploy) {
    expect(fs.readFileSync('deploy/pnpm-workspace.yaml', 'utf8')).toContain(`virtualStoreDir: ${virtualStoreDir}\n`)
  }
})

test('deploy keeps the default virtual store when virtualStoreDir names the global virtual store', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: {
        'is-positive': '1.0.0',
      },
    },
  ])

  const { allProjects, selectedProjectsGraph } =
    await filterProjectsBySelectorObjectsFromDir(process.cwd(), [{ namePattern: 'project-1' }])
  await deploy.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    lockfileDir: process.cwd(),
    workspaceDir: process.cwd(),
    enableGlobalVirtualStore: true,
    virtualStoreDir: path.resolve('global-virtual-store'),
    forceLegacyDeploy: true,
    selectedProjectsGraph,
    sharedWorkspaceLockfile: true,
  }, ['deploy'])

  expect(fs.realpathSync('deploy/node_modules/is-positive')).toBe(
    fs.realpathSync('deploy/node_modules/.pnpm/is-positive@1.0.0/node_modules/is-positive')
  )
  expect(fs.existsSync('global-virtual-store')).toBe(false)
})

test('deploy keeps the default virtual store when virtualStoreDir is absolute', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: {
        'is-positive': '1.0.0',
      },
    },
  ])

  const { allProjects, allProjectsGraph, selectedProjectsGraph } =
    await filterProjectsBySelectorObjectsFromDir(process.cwd(), [{ namePattern: 'project-1' }])
  const opts = {
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    lockfileDir: process.cwd(),
    workspaceDir: process.cwd(),
    virtualStoreDir: path.resolve('shared-virtual-store'),
  }

  await install.handler({ ...opts, allProjectsGraph, selectedProjectsGraph: allProjectsGraph })
  await deploy.handler({
    ...opts,
    selectedProjectsGraph,
    sharedWorkspaceLockfile: true,
  }, ['deploy'])

  expect(fs.realpathSync('deploy/node_modules/is-positive')).toBe(
    fs.realpathSync('deploy/node_modules/.pnpm/is-positive@1.0.0/node_modules/is-positive')
  )
  expect(fs.readFileSync('deploy/pnpm-workspace.yaml', 'utf8')).not.toContain('virtualStoreDir')
})
