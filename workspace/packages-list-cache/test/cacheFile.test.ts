import path from 'path'
import { prepareEmpty } from '@pnpm/prepare'
import { getCacheFilePath } from '../src/cacheFile'

test('getCacheFilePath()', () => {
  prepareEmpty()
  expect(
    getCacheFilePath(process.cwd())
  ).toStrictEqual(
    path.resolve(path.resolve('node_modules/.workspace-packages-list.json'))
  )
})
