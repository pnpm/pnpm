import fs from 'fs'
import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage } from '@pnpm/core'
import { testDefaults } from '../utils'

test('a package with a huge amount of circular dependencies and many peer dependencies should successfully be resolved', async () => {
  prepareEmpty()

  const registries = {
    default: 'https://registry.npmjs.org/',
    '@teambit': 'https://node-registry.bit.cloud/',
  }
  await addDependenciesToPackage({},
    ['@teambit/bit@0.0.745'],
    testDefaults({
      fastUnpack: true,
      lockfileOnly: true,
      registries,
      strictPeerDependencies: false,
    }, { registries })
  )

  expect(fs.existsSync('pnpm-lock.yaml')).toBeTruthy()
})
