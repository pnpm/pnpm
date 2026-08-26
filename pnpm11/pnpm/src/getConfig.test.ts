import { expect, test } from '@jest/globals'

import { isSingleSettingRead } from './getConfig.js'

test.each([
  ['config', ['get', 'registries'], true],
  ['config', ['get'], false],
  ['config', ['list'], false],
  ['config', ['set', 'store-dir', '/tmp/store'], false],
  ['get', ['registries'], true],
  ['get', [], false],
  ['install', [], false],
  [null, [], false],
] as Array<[string | null, string[], boolean]>)('isSingleSettingRead(%p, %p) → %p', (cmd, cliParams, expected) => {
  expect(isSingleSettingRead(cmd, cliParams)).toBe(expected)
})
