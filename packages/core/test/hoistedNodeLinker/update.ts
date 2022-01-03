import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage } from '@pnpm/core'
import { sync as loadJsonFile } from 'load-json-file'
import {
  addDistTag,
  testDefaults,
} from '../utils'

test('preserve subdeps on update', async () => {
  prepareEmpty()

  await addDistTag('foobarqar', '1.0.0', 'latest')

  const manifest = await addDependenciesToPackage(
    {},
    ['foobarqar@1.0.0', 'bar@100.1.0'],
    await testDefaults({ nodeLinker: 'hoisted' })
  )

  await addDependenciesToPackage(
    manifest,
    ['foobarqar@1.0.1'],
    await testDefaults({ nodeLinker: 'hoisted' })
  )

  expect(loadJsonFile<{ version: string }>('node_modules/foo/package.json').version).toBe('100.1.0')
  expect(loadJsonFile<{ version: string }>('node_modules/foobarqar/package.json').version).toBe('1.0.1')
  expect(loadJsonFile<{ version: string }>('node_modules/foobarqar/node_modules/foo/package.json').version).toBe('100.0.0')
})
