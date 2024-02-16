import fs from 'fs'
import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage } from '@pnpm/core'
import { isCI } from 'ci-info'
import { testDefaults } from '../utils'

const testSkipOnCI = isCI ? test.skip : test

// Looks like GitHub Actions have reduced memory limit for Node.js,
// so it fails in CI at the moment.
testSkipOnCI('a package with a huge amount of circular dependencies and many peer dependencies should successfully be resolved', async () => {
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

  expect(fs.existsSync('pnpm-lock.yaml')).toBeTruthy()
})
