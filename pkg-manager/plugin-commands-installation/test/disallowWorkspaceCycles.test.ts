import { install } from '@pnpm/plugin-commands-installation'
import { filterPackagesFromDir } from '@pnpm/workspace.filter-packages-from-dir'
import { preparePackages } from '@pnpm/prepare'
import { DEFAULT_OPTS } from './utils'
import type { PnpmError } from '@pnpm/error'

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

  const { allProjects, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [])

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

  const { allProjects, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [])

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

  const { allProjects, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [])

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
