import { whichVersionIsPinned } from '@pnpm/which-version-is-pinned'

test.each([
  ['^1.0.0', 'major'],
  ['~1.0.0', 'minor'],
  ['1.0.0', 'patch'],
  ['*', 'none'],
  ['workspace:^1.0.0', 'major'],
])('whichVersionIsPinned()', (spec, expectedResult) => {
  expect(whichVersionIsPinned(spec)).toEqual(expectedResult)
})
