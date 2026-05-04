import { expect, test } from '@jest/globals'

import { replaceVersionInBareSpecifier } from '../lib/replaceVersionInBareSpecifier.js'

test('replaceVersionInBareSpecifier()', () => {
  expect(replaceVersionInBareSpecifier('^1.0.0', '1.1.0')).toBe('1.1.0')
  expect(replaceVersionInBareSpecifier('npm:foo@^1.0.0', '1.1.0')).toBe('npm:foo@1.1.0')
  expect(replaceVersionInBareSpecifier('npm:@foo/bar@^1.0.0', '1.1.0')).toBe('npm:@foo/bar@1.1.0')
  expect(replaceVersionInBareSpecifier('npm:foo', '1.1.0')).toBe('npm:foo@1.1.0')
  expect(replaceVersionInBareSpecifier('npm:@foo/bar', '1.1.0')).toBe('npm:@foo/bar@1.1.0')
})

test('replaceVersionInBareSpecifier() applies the fast path to configured named-registry prefixes', () => {
  // The caller (deps-resolver) supplies the merged set of named-registry
  // prefixes — built-in `gh:` and any user-defined aliases — so the locked
  // version can be pasted in without re-fetching metadata.
  const prefixes = ['gh:', 'work:']
  expect(replaceVersionInBareSpecifier('gh:^1.0.0', '1.1.0', prefixes)).toBe('gh:1.1.0')
  expect(replaceVersionInBareSpecifier('gh:@acme/foo@^1.0.0', '1.1.0', prefixes)).toBe('gh:@acme/foo@1.1.0')
  expect(replaceVersionInBareSpecifier('gh:@acme/foo', '1.1.0', prefixes)).toBe('gh:@acme/foo@1.1.0')
  expect(replaceVersionInBareSpecifier('work:@corp/lib@^2.0.0', '2.1.0', prefixes)).toBe('work:@corp/lib@2.1.0')
})

test('replaceVersionInBareSpecifier() leaves unrecognized prefixes untouched', () => {
  // Other resolvers (workspace, file/link, catalog, git, tarball) own these
  // schemes; the npm-style version replacer must not rewrite them. An alias
  // that isn't in the supplied named-registry set also falls through.
  expect(replaceVersionInBareSpecifier('workspace:^1.0.0', '1.1.0')).toBe('workspace:^1.0.0')
  expect(replaceVersionInBareSpecifier('workspace:./pkg', '1.1.0')).toBe('workspace:./pkg')
  expect(replaceVersionInBareSpecifier('file:./pkg', '1.1.0')).toBe('file:./pkg')
  expect(replaceVersionInBareSpecifier('link:../pkg', '1.1.0')).toBe('link:../pkg')
  expect(replaceVersionInBareSpecifier('catalog:', '1.1.0')).toBe('catalog:')
  expect(replaceVersionInBareSpecifier('github:owner/repo', '1.1.0')).toBe('github:owner/repo')
  expect(replaceVersionInBareSpecifier('https://example.com/tarball.tgz', '1.1.0')).toBe('https://example.com/tarball.tgz')
  expect(replaceVersionInBareSpecifier('gh:^1.0.0', '1.1.0', [])).toBe('gh:^1.0.0')
  expect(replaceVersionInBareSpecifier('work:^1.0.0', '1.1.0', ['gh:'])).toBe('work:^1.0.0')
})
