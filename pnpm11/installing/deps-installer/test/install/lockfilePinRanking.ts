import { expect, test } from '@jest/globals'
import { install } from '@pnpm/installing.deps-installer'
import { prepareEmpty } from '@pnpm/prepare'

import { testDefaults } from '../utils/index.js'

// https://github.com/pnpm/pnpm/issues/17084
test.each([true, false])('a version only a replaced dependency held is not preferred after moving back, autoInstallPeers=%p', async (autoInstallPeers) => {
  const project = prepareEmpty()
  const opts = testDefaults({ autoInstallPeers, lockfileOnly: true })
  const withProvider = (provider: string) => ({
    dependencies: {
      '@pnpm.e2e/has-foo100-peer': '1.0.0',
      provider: `npm:${provider}@1.0.0`,
    },
  })

  await install(withProvider('@pnpm.e2e/has-foo-as-dep-and-subdep'), opts)
  const fresh = project.readLockfile()

  await install(withProvider('@pnpm.e2e/has-foo-100.1.0-dep-1'), opts)
  await install(withProvider('@pnpm.e2e/has-foo-as-dep-and-subdep'), opts)
  expect(project.readLockfile()).toStrictEqual(fresh)
})
