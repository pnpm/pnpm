import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage, install } from '@pnpm/core'
import { addDistTag } from '@pnpm/registry-mock'
import { testDefaults } from './utils'

test('should fail to update when requests are cached', async () => {
  const project = prepareEmpty()

  const opts = await testDefaults()

  await addDistTag({ package: 'dep-of-pkg-with-1-dep', version: '100.0.0', distTag: 'latest' })

  const manifest = await addDependenciesToPackage({}, ['pkg-with-1-dep'], opts)

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  await addDistTag({ package: 'dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  await install(manifest, { ...opts, depth: 1, update: true })

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')
})

test('should not cache when cache is not used', async () => {
  const project = prepareEmpty()

  await addDistTag({ package: 'dep-of-pkg-with-1-dep', version: '100.0.0', distTag: 'latest' })

  const manifest = await addDependenciesToPackage({}, ['pkg-with-1-dep'], await testDefaults({ save: true }))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  await addDistTag({ package: 'dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  await install(manifest, await testDefaults({ depth: 1, update: true }))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.1.0')
})
