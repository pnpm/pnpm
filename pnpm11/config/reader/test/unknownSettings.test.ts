import { expect, test } from '@jest/globals'

import { isKnownSettingKey, quoteAndAnnotateUnknown } from '../lib/unknownSettings.js'

test('quoteAndAnnotateUnknown() names the pnpm version that reads a setting this one does not', () => {
  const v12Settings = [
    'addMissingPeerTypes',
    'autoDedupe',
    'bootConfig',
    'cargo',
    'concurrencyGroups',
    'globalShims',
    'macosBackup',
    'nodePackageMap',
    'permissions',
    'pipelineBase',
    'pipelines',
    'publishWaitTimeout',
    'python',
    'remoteCache',
    'saveTypes',
    'skills',
    'storeIndexFormat',
    'tools',
  ]
  expect(v12Settings.filter(isKnownSettingKey)).toEqual([])
  expect(quoteAndAnnotateUnknown(v12Settings))
    .toBe(v12Settings.map((key) => `"${key}" (a pnpm v12 setting)`).join(', '))
})

test('quoteAndAnnotateUnknown() suggests a setting of this version for a typo', () => {
  expect(quoteAndAnnotateUnknown(['storeDur'])).toBe('"storeDur" (did you mean "storeDir"?)')
})
