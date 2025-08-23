import { filterPackagesFromDir } from '@pnpm/workspace.filter-packages-from-dir'
import { preparePackages } from '@pnpm/prepare'
import { jest } from '@jest/globals'
import { DEFAULT_OPTS } from './utils/index.js'

const warn = jest.fn()
const info = jest.fn()
const debug = jest.fn()
const original = await import('@pnpm/logger')
jest.unstable_mockModule('@pnpm/logger', () => ({
  ...original,
  logger: Object.assign(() => ({ warn, info, debug }), { warn, info, debug }),
}))
const { install } = await import('@pnpm/plugin-commands-installation')

afterEach(() => {
  jest.mocked(warn).mockRestore()
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

  const { allProjects, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  expect(warn).toHaveBeenCalledTimes(1)
  expect(warn).toHaveBeenCalledWith({
    message: expect.stringMatching(/^There are cyclic workspace dependencies: /),
    prefix: process.cwd(),
  })
})

test('should not warn about cyclic dependencies if ignore-workspace-cycles is set', async () => {
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
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
    ignoreWorkspaceCycles: true,
  })

  expect(warn).toHaveBeenCalledTimes(0)
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

  const { allProjects, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  expect(warn).toHaveBeenCalledTimes(0)
})
