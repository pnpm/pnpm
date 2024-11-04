import path from 'path'
import { logger } from '@pnpm/logger'
import { preparePackages } from '@pnpm/prepare'
import { type ProjectRootDir } from '@pnpm/types'
import { loadPackagesList, updatePackagesList } from '../src/index'

const lastValidatedTimestamp = Date.now()

const originalLoggerDebug = logger.debug
afterEach(() => {
  logger.debug = originalLoggerDebug
})

test('updatePackagesList()', async () => {
  preparePackages(['a', 'b', 'c', 'd'].map(name => ({
    location: `./packages/${name}`,
    package: { name },
  })))

  const workspaceDir = process.cwd()

  expect(await loadPackagesList(workspaceDir)).toBeUndefined()

  logger.debug = jest.fn(originalLoggerDebug)
  await updatePackagesList({
    lastValidatedTimestamp,
    workspaceDir,
    allProjects: [],
  })
  expect((logger.debug as jest.Mock).mock.calls).toStrictEqual([[{ msg: 'updating packages list' }]])
  expect(await loadPackagesList(workspaceDir)).toStrictEqual({
    lastValidatedTimestamp,
    projectRootDirs: [],
    workspaceDir,
  })

  logger.debug = jest.fn(originalLoggerDebug)
  await updatePackagesList({
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
  expect((logger.debug as jest.Mock).mock.calls).toStrictEqual([[{ msg: 'updating packages list' }]])
  expect(await loadPackagesList(workspaceDir)).toStrictEqual({
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
    workspaceDir,
  })
})
