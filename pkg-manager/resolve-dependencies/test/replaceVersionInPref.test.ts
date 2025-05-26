import { replaceVersionInBareSpecifier } from '../lib/replaceVersionInBareSpecifier'

test('replaceVersionInBareSpecifier()', () => {
  expect(replaceVersionInBareSpecifier('^1.0.0', '1.1.0')).toBe('1.1.0')
  expect(replaceVersionInBareSpecifier('npm:foo@^1.0.0', '1.1.0')).toBe('npm:foo@1.1.0')
  expect(replaceVersionInBareSpecifier('npm:@foo/bar@^1.0.0', '1.1.0')).toBe('npm:@foo/bar@1.1.0')
  expect(replaceVersionInBareSpecifier('npm:foo', '1.1.0')).toBe('npm:foo@1.1.0')
  expect(replaceVersionInBareSpecifier('npm:@foo/bar', '1.1.0')).toBe('npm:@foo/bar@1.1.0')
})
