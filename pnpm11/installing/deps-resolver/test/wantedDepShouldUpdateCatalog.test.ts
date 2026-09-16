import { expect, test } from '@jest/globals'

import { wantedDepShouldUpdateCatalog } from '../src/wantedDepShouldUpdateCatalog.js'

test('catalog updates require a persistable updated specifier', () => {
  expect(wantedDepShouldUpdateCatalog(undefined)).toBe(false)
  expect(wantedDepShouldUpdateCatalog({ updateSpec: false })).toBe(false)
  expect(wantedDepShouldUpdateCatalog({ updateSpec: true })).toBe(true)
  expect(wantedDepShouldUpdateCatalog({ saveSpec: false, updateSpec: true })).toBe(false)
})
