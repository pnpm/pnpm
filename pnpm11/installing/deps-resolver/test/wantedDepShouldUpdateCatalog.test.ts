import { expect, test } from '@jest/globals'

import { wantedDepShouldUpdateCatalog } from '../src/wantedDepShouldUpdateCatalog.js'

test('catalog updates require an updated specifier that can be saved', () => {
  expect(wantedDepShouldUpdateCatalog(undefined)).toBe(false)
  expect(wantedDepShouldUpdateCatalog({ updateSpec: false })).toBe(false)
  expect(wantedDepShouldUpdateCatalog({ updateSpec: true })).toBe(true)
  expect(wantedDepShouldUpdateCatalog({ saveSpec: false, updateSpec: true })).toBe(false)
})
