import { promises as fs } from 'fs'
import path from 'path'
import createImportPackage from '@pnpm/package-store/lib/storeController/createImportPackage'
import { prepareEmpty } from '@pnpm/prepare'

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
