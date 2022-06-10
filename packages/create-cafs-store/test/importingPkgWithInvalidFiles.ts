import { promises as fs } from 'fs'
import path from 'path'
import { prepareEmpty } from '@pnpm/prepare'
import createImportPackage from '../lib/createImportPackage'

test('importing a package with invalid files', async () => {
  prepareEmpty()
  const importPackage = createImportPackage('copy')
  const target = path.resolve('target')
  await importPackage(target, {
    filesMap: {
      'foo?bar/qar>zoo.txt': __filename,
      '1*2.txt': __filename,
    },
    force: false,
    fromStore: false,
  })
  expect((await fs.readdir(target)).length).toBe(2)
})
