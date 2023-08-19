import fs from 'fs'
import path from 'path'
import { prepareEmpty } from '@pnpm/prepare'
import { createIndexedPkgImporter } from '@pnpm/fs.indexed-pkg-importer'

test('importing a package with invalid files', () => {
  prepareEmpty()
  const importPackage = createIndexedPkgImporter('copy')
  const target = path.resolve('target')
  importPackage(target, {
    filesMap: {
      'foo?bar/qar>zoo.txt': __filename,
      '1*2.txt': __filename,
    },
    force: false,
    fromStore: false,
  })
  expect((fs.readdirSync(target)).length).toBe(2)
})
