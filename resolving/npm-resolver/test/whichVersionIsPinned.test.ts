import { whichVersionIsPinned } from '../lib/whichVersionIsPinned'

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
  ['jsr:@foo/foo@1.0.0', 'patch'],
  ['jsr:foo@^1.0.0', 'major'],
])('whichVersionIsPinned()', (spec, expectedResult) => {
  expect(whichVersionIsPinned(spec)).toEqual(expectedResult)
})
