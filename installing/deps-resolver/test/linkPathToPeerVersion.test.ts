import { expect, test } from '@jest/globals'

import { linkPathToPeerVersion } from '../lib/linkPathToPeerVersion.js'

// These outputs are lockfile-format: changing any of them breaks existing
// v9 lockfiles. See https://github.com/pnpm/pnpm/issues/11272.
test.each([
  // The case from #11272: link target outside the workspace root.
  ['../packages/b', 'packages+b'],
  ['./packages/b', 'packages+b'],
  ['packages/b', 'packages+b'],
  ['../../a/b', '..+a+b'],
  ['a/b/c', 'a+b+c'],
  ['abc', 'abc'],
  // Leading dots collapse and are stripped.
  ['..', '+'],
  ['...', '+'],
  ['.hidden/pkg', 'hidden+pkg'],
  // Windows-style separators and mixed reserved characters.
  ['..\\packages\\b', 'packages+b'],
  ['a/b\\c', 'a+b+c'],
  // Literal '+' characters collapse with adjacent separators.
  ['foo+bar', 'foo+bar'],
  ['foo++bar', 'foo+bar'],
  ['+foo', 'foo'],
  ['foo+', 'foo'],
  // Trailing dots are stripped.
  ['foo.', 'foo'],
  ['abc...', 'abc'],
  // Empty input stays empty.
  ['', ''],
])('linkPathToPeerVersion(%j) === %j', (input, expected) => {
  expect(linkPathToPeerVersion(input)).toBe(expected)
})
