import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage } from '@pnpm/core'
import exists from 'path-exists'
import { testDefaults } from '../utils'

test('a package with a huge amount of circular dependencies and many peer dependencies should succesfully be resolved', async () => {
  prepareEmpty()

  await addDependenciesToPackage({},
    ['@teambit/bit@0.0.745'],
    await testDefaults({
      fastUnpack: true,
      lockfileOnly: true,
      registries: {
        '@teambit': 'https://node.bit.dev/',
      },
      strictPeerDependencies: false,
    })
  )

  expect(await exists('pnpm-lock.yaml')).toBeTruthy()
})
