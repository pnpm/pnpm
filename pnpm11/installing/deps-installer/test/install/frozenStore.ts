import { expect, test } from '@jest/globals'
import { install } from '@pnpm/installing.deps-installer'
import { prepareEmpty } from '@pnpm/prepare'

import { testDefaults } from '../utils/index.js'

test('frozenStore together with force throws a config conflict error', async () => {
  prepareEmpty()
  await expect(
    install({}, testDefaults({
      frozenStore: true,
      force: true,
    }))
  ).rejects.toMatchObject({
    code: 'ERR_PNPM_CONFIG_CONFLICT_FROZEN_STORE_WITH_FORCE',
  })
})

test('frozenStore together with a configured pnpr server throws before any store write', async () => {
  prepareEmpty()
  await expect(
    install({}, testDefaults({
      frozenStore: true,
      pnprServer: 'http://localhost:0',
    }))
  ).rejects.toMatchObject({
    code: 'ERR_PNPM_FROZEN_STORE_INCOMPATIBLE_WITH_PNPR',
  })
})
