import { whichVersionIsPinned } from '@pnpm/which-version-is-pinned'

test.each([
  ['^1.0.0', 'major'],
  ['~1.0.0', 'minor'],
  ['1.0.0', 'patch'],
  ['*', 'none'],
  ['workspace:^1.0.0', 'major'],
  ['npm:foo@1.0.0', 'patch'],
  ['npm:@foo/foo@1.0.0', 'patch'],
  ['npm:foo@^1.0.0', 'major'],
  ['npm:@foo/foo@^1.0.0', 'major'],
  ['npm:@pnpm.e2e/qar@100.0.0', 'patch'],
])('whichVersionIsPinned()', (spec, expectedResult) => {
  expect(whichVersionIsPinned(spec)).toEqual(expectedResult)
})
