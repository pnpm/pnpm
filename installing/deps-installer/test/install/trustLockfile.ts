import { expect, test } from '@jest/globals'
import { install } from '@pnpm/installing.deps-installer'
import { prepareEmpty } from '@pnpm/prepare'
import type { ResolutionVerifier } from '@pnpm/resolving.resolver-base'

import { testDefaults } from '../utils/index.js'

const rejectingVerifier: ResolutionVerifier = {
  policy: {},
  canTrustPastCheck: () => false,
  verify: async (_resolution, { name, version }) => ({
    ok: false,
    code: 'TEST_REJECT',
    reason: `${name}@${version} rejected by test verifier`,
  }),
}

test('install rejects the lockfile when a verifier fails and trustLockfile is unset', async () => {
  prepareEmpty()

  await install(
    { dependencies: { 'is-positive': '1.0.0' } },
    testDefaults()
  )

  await expect(
    install(
      { dependencies: { 'is-positive': '1.0.0' } },
      testDefaults({
        frozenLockfile: true,
        resolutionVerifiers: [rejectingVerifier],
      })
    )
  ).rejects.toMatchObject({ code: 'ERR_PNPM_TEST_REJECT' })
})

test('install skips lockfile verification when trustLockfile is true even if a verifier rejects', async () => {
  prepareEmpty()

  await install(
    { dependencies: { 'is-positive': '1.0.0' } },
    testDefaults()
  )

  await expect(
    install(
      { dependencies: { 'is-positive': '1.0.0' } },
      testDefaults({
        frozenLockfile: true,
        trustLockfile: true,
        resolutionVerifiers: [rejectingVerifier],
      })
    )
  ).resolves.toBeDefined()
})
