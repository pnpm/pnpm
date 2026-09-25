import { expect, test } from '@jest/globals'

import { getStaleOverrideTargets } from '../lib/install/getStaleOverrideTargets.js'

test('a removed unscoped selector targets its package', () => {
  expect(getStaleOverrideTargets({ foo: '1.0.0', '@scope/bar': '2.0.0' }, {})).toEqual(new Set(['foo', '@scope/bar']))
})

test('a selector that is still set, even with a new value, targets nothing', () => {
  expect(getStaleOverrideTargets({ foo: '1.0.0' }, { foo: '1.1.0' })).toEqual(new Set())
})

test('a removed selector scoped to a parent or a version range targets nothing', () => {
  expect(getStaleOverrideTargets({ 'qar@1>foo': '1.0.0', 'foo@^1': '1.0.0', 'qar>foo': '1.0.0' }, {})).toEqual(new Set())
})

test('a selector that cannot be parsed is skipped', () => {
  expect(getStaleOverrideTargets({ '': '1.0.0', foo: '1.0.0' }, {})).toEqual(new Set(['foo']))
})

test('a lockfile without overrides targets nothing', () => {
  expect(getStaleOverrideTargets(undefined, { foo: '1.0.0' })).toEqual(new Set())
})
