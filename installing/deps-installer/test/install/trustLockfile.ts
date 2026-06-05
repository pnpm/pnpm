import fs from 'node:fs'

import { expect, test } from '@jest/globals'
import { addDependenciesToPackage, install } from '@pnpm/installing.deps-installer'
import { prepareEmpty } from '@pnpm/prepare'
import type { ResolutionVerifier } from '@pnpm/resolving.resolver-base'
import { rimrafSync } from '@zkochan/rimraf'

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

test('dependency lifecycle scripts do not run when lockfile verification fails', async () => {
  prepareEmpty()

  const pkgName = '@pnpm.e2e/pre-and-postinstall-scripts-example'
  const { updatedManifest: manifest } = await addDependenciesToPackage({},
    [`${pkgName}@1.0.0`],
    testDefaults({ fastUnpack: false, allowBuilds: { [pkgName]: true } })
  )
  rimrafSync('node_modules')

  await expect(
    install(manifest, testDefaults({
      fastUnpack: false,
      frozenLockfile: true,
      allowBuilds: { [pkgName]: true },
      resolutionVerifiers: [rejectingVerifier],
    }))
  ).rejects.toMatchObject({ code: 'ERR_PNPM_TEST_REJECT' })

  // The postinstall script is what writes this file; its absence proves the
  // build phase was gated behind the (failed) verification.
  expect(fs.existsSync(`node_modules/${pkgName}/generated-by-postinstall.js`)).toBeFalsy()
})
