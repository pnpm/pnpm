import path from 'path'
import { logger } from '@pnpm/logger'
import { preparePackages } from '@pnpm/prepare'
import { type ProjectRootDir } from '@pnpm/types'
import { loadWorkspaceState, updateWorkspaceState } from '../src/index'

const lastValidatedTimestamp = Date.now()

const originalLoggerDebug = logger.debug
afterEach(() => {
  logger.debug = originalLoggerDebug
})

test('updateWorkspaceState()', async () => {
  preparePackages(['a', 'b', 'c', 'd'].map(name => ({
    location: `./packages/${name}`,
    package: { name },
  })))

  const workspaceDir = process.cwd()

  expect(loadWorkspaceState(workspaceDir)).toBeUndefined()

  logger.debug = jest.fn(originalLoggerDebug)
  await updateWorkspaceState({
    lastValidatedTimestamp,
    workspaceDir,
    catalogs: undefined,
    allProjects: [],
  })
  expect((logger.debug as jest.Mock).mock.calls).toStrictEqual([[{ msg: 'updating workspace state' }]])
  expect(loadWorkspaceState(workspaceDir)).toStrictEqual({
    lastValidatedTimestamp,
    projectRootDirs: [],
  })

  logger.debug = jest.fn(originalLoggerDebug)
  await updateWorkspaceState({
    lastValidatedTimestamp,
    workspaceDir,
    catalogs: {
      default: {
        foo: '0.1.2',
      },
    },
    allProjects: [
      { rootDir: path.resolve('packages/c') as ProjectRootDir },
      { rootDir: path.resolve('packages/a') as ProjectRootDir },
      { rootDir: path.resolve('packages/d') as ProjectRootDir },
      { rootDir: path.resolve('packages/b') as ProjectRootDir },
    ],
  })
  expect((logger.debug as jest.Mock).mock.calls).toStrictEqual([[{ msg: 'updating workspace state' }]])
  expect(loadWorkspaceState(workspaceDir)).toStrictEqual({
    catalogs: {
      default: {
        foo: '0.1.2',
      },
    },
    lastValidatedTimestamp,
    projectRootDirs: [
      path.resolve('packages/a'),
      path.resolve('packages/b'),
      path.resolve('packages/c'),
      path.resolve('packages/d'),
    ],
  })
})
