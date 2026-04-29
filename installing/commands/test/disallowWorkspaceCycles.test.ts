import { expect, test } from '@jest/globals'
import type { PnpmError } from '@pnpm/error'
import { install } from '@pnpm/installing.commands'
import { preparePackages } from '@pnpm/prepare'
import { filterProjectsBySelectorObjectsFromDir } from '@pnpm/workspace.projects-filter'

import { DEFAULT_OPTS } from './utils/index.js'

test('should error if disallow-workspace-cycles is set', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: { 'project-2': 'workspace:*' },
    },
    {
      name: 'project-2',
      version: '2.0.0',
      devDependencies: { 'project-1': 'workspace:*' },
    },
  ])

  const { allProjects, selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])

  let err!: PnpmError
  try {
    await install.handler({
      ...DEFAULT_OPTS,
      allProjects,
      dir: process.cwd(),
      recursive: true,
      selectedProjectsGraph,
      workspaceDir: process.cwd(),
      disallowWorkspaceCycles: true,
    })
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err.code).toBe('ERR_PNPM_DISALLOW_WORKSPACE_CYCLES')
})

test('should not error if disallow-workspace-cycles is not set', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: { 'project-2': 'workspace:*' },
    },
    {
      name: 'project-2',
      version: '2.0.0',
      devDependencies: { 'project-1': 'workspace:*' },
    },
  ])

  const { allProjects, selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])

  let err!: PnpmError
  try {
    await install.handler({
      ...DEFAULT_OPTS,
      allProjects,
      dir: process.cwd(),
      recursive: true,
      selectedProjectsGraph,
      workspaceDir: process.cwd(),
    })
  } catch (_err: any) {  // eslint-disable-line
    err = _err
  }
  expect(err).toBeUndefined()
})

test('should not error if there are no cyclic dependencies', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: { 'project-2': 'workspace:*' },
    },
    {
      name: 'project-2',
      version: '2.0.0',
    },
  ])

  const { allProjects, selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])

  let err!: PnpmError
  try {
    await install.handler({
      ...DEFAULT_OPTS,
      allProjects,
      dir: process.cwd(),
      recursive: true,
      selectedProjectsGraph,
      workspaceDir: process.cwd(),
      disallowWorkspaceCycles: true,
    })
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err).toBeUndefined()
})
