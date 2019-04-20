import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage, install } from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import {
  addDistTag,
  testDefaults,
} from './utils'

const test = promisifyTape(tape)

test('should fail to update when requests are cached', async (t) => {
  const project = prepareEmpty(t)

  const metaCache = new Map()

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', 'latest')

  const manifest = await addDependenciesToPackage({}, ['pkg-with-1-dep'], await testDefaults({ save: true, metaCache }))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')

  await install(manifest, await testDefaults({ depth: 1, metaCache, update: true }))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')
})

test('should not cache when cache is not used', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', 'latest')

  const manifest = await addDependenciesToPackage({}, ['pkg-with-1-dep'], await testDefaults({ save: true }))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')

  await install(manifest, await testDefaults({ depth: 1, update: true }))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.1.0')
})
