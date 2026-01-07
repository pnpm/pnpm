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
      'foo?bar/qar>zoo.txt': import.meta.filename,
      '1*2.txt': import.meta.filename,
    },
    force: false,
    resolvedFrom: 'remote',
  })
  expect((fs.readdirSync(target))).toHaveLength(2)
})
