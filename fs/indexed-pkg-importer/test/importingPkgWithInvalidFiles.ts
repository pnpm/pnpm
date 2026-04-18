import fs from 'node:fs'
import path from 'node:path'

import { createIndexedPkgImporter } from '@pnpm/fs.indexed-pkg-importer'
import { prepareEmpty } from '@pnpm/prepare'

test('importing a package with invalid files', async () => {
  prepareEmpty()
  const importPackage = createIndexedPkgImporter('copy')
  const target = path.resolve('target')
  await importPackage(target, {
    filesMap: new Map([
      ['foo?bar/qar>zoo.txt', import.meta.filename],
      ['1*2.txt', import.meta.filename],
    ]),
    force: false,
    resolvedFrom: 'remote',
  })
  console.log('target contents:', fs.readdirSync(target));
  try {
    console.log('foo?bar:', fs.readdirSync(path.join(target, 'foo?bar')));
  } catch (_e) {}
  expect((fs.readdirSync(target))).toHaveLength(2)
})
