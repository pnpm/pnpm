import { type ProjectRootDir } from '@pnpm/types'
import { createPackagesList } from '../src/createPackagesList'

test('createPackagesList() on empty list', () => {
  expect(
    createPackagesList({
      allProjects: [],
      workspaceDir: '/home/user/repos/my-project',
    })
  ).toStrictEqual({
    projectRootDirs: [],
    workspaceDir: '/home/user/repos/my-project',
  })
})

test('createPackagesList() on non-empty list', () => {
  expect(
    createPackagesList({
      allProjects: [
        { rootDir: '/home/user/repos/my-project/packages/c' as ProjectRootDir },
        { rootDir: '/home/user/repos/my-project/packages/a' as ProjectRootDir },
        { rootDir: '/home/user/repos/my-project/packages/d' as ProjectRootDir },
        { rootDir: '/home/user/repos/my-project/packages/b' as ProjectRootDir },
      ],
      workspaceDir: '/home/user/repos/my-project',
    })
  ).toStrictEqual({
    projectRootDirs: [
      '/home/user/repos/my-project/packages/a',
      '/home/user/repos/my-project/packages/b',
      '/home/user/repos/my-project/packages/c',
      '/home/user/repos/my-project/packages/d',
    ],
    workspaceDir: '/home/user/repos/my-project',
  })
})
