import { expect, test } from '@jest/globals'

import { replaceVersionInBareSpecifier } from '../lib/replaceVersionInBareSpecifier.js'

test('replaceVersionInBareSpecifier()', () => {
  expect(replaceVersionInBareSpecifier('^1.0.0', '1.1.0')).toBe('1.1.0')
  expect(replaceVersionInBareSpecifier('npm:foo@^1.0.0', '1.1.0')).toBe('npm:foo@1.1.0')
  expect(replaceVersionInBareSpecifier('npm:@foo/bar@^1.0.0', '1.1.0')).toBe('npm:@foo/bar@1.1.0')
  expect(replaceVersionInBareSpecifier('npm:foo', '1.1.0')).toBe('npm:foo@1.1.0')
  expect(replaceVersionInBareSpecifier('npm:@foo/bar', '1.1.0')).toBe('npm:@foo/bar@1.1.0')
})

test('replaceVersionInBareSpecifier() handles the built-in gh: prefix so the locked-version fast path works', () => {
  // Without this, `gh:^1.0.0` stays unchanged across re-resolution and defeats
  // the metadata-fetch skip that `npm:` already gets.
  expect(replaceVersionInBareSpecifier('gh:^1.0.0', '1.1.0')).toBe('gh:1.1.0')
  expect(replaceVersionInBareSpecifier('gh:@acme/foo@^1.0.0', '1.1.0')).toBe('gh:@acme/foo@1.1.0')
  expect(replaceVersionInBareSpecifier('gh:@acme/foo', '1.1.0')).toBe('gh:@acme/foo@1.1.0')
})

test('replaceVersionInBareSpecifier() leaves non-registry prefixes untouched', () => {
  // These prefixes are handled by other resolvers (local, git, catalog, jsr) and
  // must not be rewritten by the npm-style version replacer.
  expect(replaceVersionInBareSpecifier('workspace:^1.0.0', '1.1.0')).toBe('workspace:^1.0.0')
  expect(replaceVersionInBareSpecifier('workspace:./pkg', '1.1.0')).toBe('workspace:./pkg')
  expect(replaceVersionInBareSpecifier('file:./pkg', '1.1.0')).toBe('file:./pkg')
  expect(replaceVersionInBareSpecifier('link:../pkg', '1.1.0')).toBe('link:../pkg')
  expect(replaceVersionInBareSpecifier('catalog:', '1.1.0')).toBe('catalog:')
  expect(replaceVersionInBareSpecifier('github:owner/repo', '1.1.0')).toBe('github:owner/repo')
  expect(replaceVersionInBareSpecifier('https://example.com/tarball.tgz', '1.1.0')).toBe('https://example.com/tarball.tgz')
  // User-defined named-registry aliases are not in the fast-path list — keeping
  // them untouched just means one extra metadata fetch on re-resolution.
  expect(replaceVersionInBareSpecifier('work:^1.0.0', '1.1.0')).toBe('work:^1.0.0')
})
