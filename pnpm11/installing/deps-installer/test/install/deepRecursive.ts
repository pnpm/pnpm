import fs from 'node:fs'

import { expect, test } from '@jest/globals'
import { addDependenciesToPackage } from '@pnpm/installing.deps-installer'
import { prepareEmpty } from '@pnpm/prepare'

import { testDefaults } from '../utils/index.js'

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
      // The tree resolves against the live registry. @yarnpkg/core@4.9.2 was
      // published with a yarn-only "patch:" specifier for got.
      overrides: {
        '@yarnpkg/core@4': '4.9.1',
      },
      registriesByScope: registries,
      strictPeerDependencies: false,
    }, { registriesByScope: registries })
  )

  expect(fs.existsSync('pnpm-lock.yaml')).toBeTruthy()
})
