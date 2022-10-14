import { install } from '@pnpm/plugin-commands-installation'
import { readProjects } from '@pnpm/filter-workspace-packages'
import { preparePackages } from '@pnpm/prepare'
import { logger } from '@pnpm/logger'
import { DEFAULT_OPTS } from './utils'

beforeEach(() => {
  jest.spyOn(logger, 'warn')
})

afterEach(() => {
  (logger.warn as jest.Mock).mockRestore()
})

test('should warn about cyclic dependencies', async () => {
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

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  expect(logger.warn).toHaveBeenCalledTimes(1)
  expect(logger.warn).toHaveBeenCalledWith({
    message: expect.stringMatching(/^There are cyclic workspace dependencies: /),
    prefix: process.cwd(),
  })
})

test('should not warn about cyclic dependencies if there are not', async () => {
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

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  expect(logger.warn).toHaveBeenCalledTimes(0)
})
