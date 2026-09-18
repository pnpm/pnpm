import { expect, test } from '@jest/globals'

import { quoteAndAnnotateUnknown } from '../lib/unknownSettings.js'

test('quoteAndAnnotateUnknown() names the pnpm version that reads a setting this one does not', () => {
  expect(quoteAndAnnotateUnknown(['cargo', 'concurrencyGroups', 'globalShims', 'pipelines']))
    .toBe('"cargo" (a pnpm v12 setting), "concurrencyGroups" (a pnpm v12 setting), "globalShims" (a pnpm v12 setting), "pipelines" (a pnpm v12 setting)')
})

test('quoteAndAnnotateUnknown() suggests a setting of this version for a typo', () => {
  expect(quoteAndAnnotateUnknown(['storeDur'])).toBe('"storeDur" (did you mean "storeDir"?)')
})
