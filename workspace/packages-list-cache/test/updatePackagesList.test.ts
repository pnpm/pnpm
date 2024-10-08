import path from 'path'
import { prepareEmpty } from '@pnpm/prepare'
import { type ProjectRootDir } from '@pnpm/types'
import { loadPackagesList, updatePackagesList } from '../src/index'

test('updatePackagesList()', async () => {
  prepareEmpty()

  const cacheDir = path.resolve('cache')
  const workspaceDir = process.cwd()

  expect(await loadPackagesList({ cacheDir, workspaceDir })).toBeUndefined()

  await updatePackagesList({
    cacheDir,
    workspaceDir,
    allProjects: [],
  })
  expect(await loadPackagesList({ cacheDir, workspaceDir })).toStrictEqual({
    projectRootDirs: [],
    workspaceDir,
  })

  await updatePackagesList({
    cacheDir,
    workspaceDir,
    allProjects: [
      { rootDir: '/home/user/repos/my-project/packages/c' as ProjectRootDir },
      { rootDir: '/home/user/repos/my-project/packages/a' as ProjectRootDir },
      { rootDir: '/home/user/repos/my-project/packages/d' as ProjectRootDir },
      { rootDir: '/home/user/repos/my-project/packages/b' as ProjectRootDir },
    ],
  })
  expect(await loadPackagesList({ cacheDir, workspaceDir })).toStrictEqual({
    projectRootDirs: [
      '/home/user/repos/my-project/packages/a',
      '/home/user/repos/my-project/packages/b',
      '/home/user/repos/my-project/packages/c',
      '/home/user/repos/my-project/packages/d',
    ],
    workspaceDir,
  })
})
