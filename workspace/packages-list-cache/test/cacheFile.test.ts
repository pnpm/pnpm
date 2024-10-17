import path from 'path'
import { getCacheFilePath } from '../src/cacheFile'

test('getCacheFilePath()', () => {
  expect(
    getCacheFilePath({
      cacheDir: path.resolve('cache'),
      workspaceDir: '/home/user/repos/my-project',
    })
  ).toStrictEqual(
    path.resolve('cache/workspace-packages-lists/v1/0d2aa676fbaba5bfb6c3d15fd1ff4e52.json')
  )
})
