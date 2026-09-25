import { expect, test } from '@jest/globals'

import { inferRangeSpecStyle } from '../lib/inferRangeSpecStyle.js'

test.each([
  ['^1.0.0', 'major'],
  ['~1.0.0', 'minor'],
  ['1.0.0', 'patch'],
  ['=1.0.0', 'exact'],
  ['=1.0', 'minor'],
  ['=1', 'major'],
  ['>=1.0.0', undefined],
  ['^=1.0.0', undefined],
  ['~=1.0.0', undefined],
  ['=1.0.0||', undefined],
  ['1.0.0||', undefined],
  ['*', 'none'],
  ['workspace:^1.0.0', 'major'],
  ['npm:foo@1.0.0', 'patch'],
  ['npm:@foo/foo@1.0.0', 'patch'],
  ['npm:foo@=1.0.0', 'exact'],
  ['npm:foo@^1.0.0', 'major'],
  ['npm:@foo/foo@^1.0.0', 'major'],
  ['npm:@pnpm.e2e/qar@100.0.0', 'patch'],
  ['jsr:@foo/foo@1.0.0', 'patch'],
  ['jsr:foo@^1.0.0', 'major'],
  ['catalog:', undefined],
  ['catalog:default', undefined],
  ['catalog:foo', undefined],
  // A catalog name that parses as a version must not be treated as a pin.
  ['catalog:express4-21', undefined],
])('inferRangeSpecStyle()', (spec, expectedResult) => {
  expect(inferRangeSpecStyle(spec)).toEqual(expectedResult)
})
