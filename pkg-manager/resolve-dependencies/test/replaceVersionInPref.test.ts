import { replaceVersionInPref } from '../lib/replaceVersionInPref'

test('replaceVersionInPref()', () => {
  expect(replaceVersionInPref('^1.0.0', '1.1.0')).toBe('1.1.0')
  expect(replaceVersionInPref('npm:foo@^1.0.0', '1.1.0')).toBe('npm:foo@1.1.0')
  expect(replaceVersionInPref('npm:@foo/bar@^1.0.0', '1.1.0')).toBe('npm:@foo/bar@1.1.0')
  expect(replaceVersionInPref('npm:foo', '1.1.0')).toBe('npm:foo@1.1.0')
  expect(replaceVersionInPref('npm:@foo/bar', '1.1.0')).toBe('npm:@foo/bar@1.1.0')
})
