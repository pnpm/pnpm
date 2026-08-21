import { expect, test } from '@jest/globals'
import {
  DIRECT_DEP_SELECTOR_WEIGHT,
  EXISTING_VERSION_SELECTOR_WEIGHT,
} from '@pnpm/resolving.resolver-base'

import {
  createPreferredVersionsFromPinnedUpdateSpecs,
  createUpdateMatching,
} from '../lib/recursive.js'

test('createUpdateMatching() does not match other major versions for pinned selectors', () => {
  const updateMatching = createUpdateMatching(['js-yaml@3.15.1'])

  expect(updateMatching('js-yaml', '3.15.0')).toBeTruthy()
  expect(updateMatching('js-yaml', '3.15.1')).toBeTruthy()
  expect(updateMatching('js-yaml', '4.3.0')).toBeFalsy()
})

test('createUpdateMatching() keeps 0.x selectors scoped by minor line', () => {
  const updateMatching = createUpdateMatching(['foo@0.2.5'])

  expect(updateMatching('foo', '0.2.1')).toBeTruthy()
  expect(updateMatching('foo', '0.3.0')).toBeFalsy()
  expect(updateMatching('foo', '1.0.0')).toBeFalsy()
})

test('createPreferredVersionsFromPinnedUpdateSpecs() seeds exact and cap selectors', () => {
  const preferredVersions = createPreferredVersionsFromPinnedUpdateSpecs(['js-yaml@3.15.1'])

  expect(preferredVersions).toBeTruthy()
  expect(preferredVersions?.['js-yaml']?.['3.15.1']).toStrictEqual({
    selectorType: 'version',
    weight: EXISTING_VERSION_SELECTOR_WEIGHT + DIRECT_DEP_SELECTOR_WEIGHT + 1,
  })
  expect(preferredVersions?.['js-yaml']?.['<=3.15.1']).toStrictEqual({
    selectorType: 'range',
    weight: DIRECT_DEP_SELECTOR_WEIGHT + 1,
  })
})

test('createPreferredVersionsFromPinnedUpdateSpecs() ignores non-exact and negated patterns', () => {
  const preferredVersions = createPreferredVersionsFromPinnedUpdateSpecs([
    '!js-yaml@3.15.1',
    'js-yaml@^3.15.1',
    'js-yaml@latest',
    'js-yaml*',
  ])

  expect(preferredVersions).toBeUndefined()
})
