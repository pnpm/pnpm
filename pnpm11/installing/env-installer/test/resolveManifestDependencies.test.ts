import { expect, test } from '@jest/globals'
import { prepareEmpty } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
import { createTempStore } from '@pnpm/testing.temp-store'

import { resolveManifestDependencies } from '../src/resolveManifestDependencies.js'

const registry = `http://localhost:${REGISTRY_MOCK_PORT}/npm/`

// `0.0.5` was published without the provenance an earlier version carries.
const downgradedTrust = { dependencies: { '@pnpm/e2e.test-provenance': '0.0.5' } }

test('the trust policy holds the resolution to the trust evidence of earlier versions', async () => {
  prepareEmpty()
  const { storeController, storeDir } = createTempStore({ clientOptions: { fullMetadata: true } })
  const opts = {
    dir: process.cwd(),
    registriesByScope: { default: registry },
    storeController,
    storeDir,
  }

  await expect(resolveManifestDependencies(downgradedTrust, { ...opts, trustPolicy: 'no-downgrade' }))
    .rejects.toHaveProperty('code', 'ERR_PNPM_TRUST_DOWNGRADE')
  await expect(resolveManifestDependencies(downgradedTrust, {
    ...opts,
    trustPolicy: 'no-downgrade',
    trustPolicyExclude: ['@pnpm/e2e.test-provenance'],
  })).resolves.toBeDefined()
  await expect(resolveManifestDependencies(downgradedTrust, opts)).resolves.toBeDefined()
})
