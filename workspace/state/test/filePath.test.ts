import path from 'path'
import { prepareEmpty } from '@pnpm/prepare'
import { getFilePath } from '../src/filePath'

test('getFilePath()', () => {
  prepareEmpty()
  expect(
    getFilePath(process.cwd())
  ).toStrictEqual(
    path.resolve(path.resolve('node_modules/.workspace-packages-list.json'))
  )
})
