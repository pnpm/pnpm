import { expect, test } from '@jest/globals'

import { lockedVersionSatisfies } from '../lib/dependencyManifest.js'

test('lockedVersionSatisfies() checks the package a named-registry spec names', () => {
  const locked = { name: 'bar', version: '1.2.0', registryName: 'work' }
  expect(lockedVersionSatisfies({ alias: 'foo', bareSpecifier: 'work:bar@^1.0.0' }, locked)).toBe(true)
  expect(lockedVersionSatisfies({ alias: 'foo', bareSpecifier: 'work:baz@^1.0.0' }, locked)).toBe(false)
  expect(lockedVersionSatisfies({ alias: 'foo', bareSpecifier: 'work:^1.0.0' }, locked)).toBe(false)
  expect(lockedVersionSatisfies({ alias: 'bar', bareSpecifier: 'work:^1.0.0' }, locked)).toBe(true)
})

test('lockedVersionSatisfies() cannot judge a spec that is not a range', () => {
  expect(lockedVersionSatisfies({ alias: 'foo', bareSpecifier: 'latest' }, { name: 'foo', version: '1.0.0' })).toBeUndefined()
})
