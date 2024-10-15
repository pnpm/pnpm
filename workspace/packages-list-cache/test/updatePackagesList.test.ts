import path from 'path'
import { preparePackages } from '@pnpm/prepare'
import { type ProjectRootDir } from '@pnpm/types'
import { loadPackagesList, updatePackagesList } from '../src/index'

const lastValidatedTimestamp = Date.now()

test('updatePackagesList()', async () => {
  preparePackages(['a', 'b', 'c', 'd'].map(name => ({
    location: `./packages/${name}`,
    package: { name },
  })))

  const cacheDir = path.resolve('cache')
  const workspaceDir = process.cwd()

  expect(await loadPackagesList({ cacheDir, workspaceDir })).toBeUndefined()

  await updatePackagesList({
    cacheDir,
    lastValidatedTimestamp,
    workspaceDir,
    allProjects: [],
  })
  expect(await loadPackagesList({ cacheDir, workspaceDir })).toStrictEqual({
    lastValidatedTimestamp,
    projectRootDirs: [],
    workspaceDir,
  })

  await updatePackagesList({
    cacheDir,
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
  expect(await loadPackagesList({ cacheDir, workspaceDir })).toStrictEqual({
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
