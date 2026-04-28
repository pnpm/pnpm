import { expect, test } from '@jest/globals'

import { replaceVersionInBareSpecifier } from '../lib/replaceVersionInBareSpecifier.js'

test('replaceVersionInBareSpecifier()', () => {
  expect(replaceVersionInBareSpecifier('^1.0.0', '1.1.0')).toBe('1.1.0')
  expect(replaceVersionInBareSpecifier('npm:foo@^1.0.0', '1.1.0')).toBe('npm:foo@1.1.0')
  expect(replaceVersionInBareSpecifier('npm:@foo/bar@^1.0.0', '1.1.0')).toBe('npm:@foo/bar@1.1.0')
  expect(replaceVersionInBareSpecifier('npm:foo', '1.1.0')).toBe('npm:foo@1.1.0')
  expect(replaceVersionInBareSpecifier('npm:@foo/bar', '1.1.0')).toBe('npm:@foo/bar@1.1.0')
})

test('replaceVersionInBareSpecifier() leaves non-npm: prefixes untouched', () => {
  // Anything other than the standard npm: protocol falls through to its own
  // resolver. Named-registry aliases (built-in `gh:` or user-defined `work:`)
  // skip this fast path on purpose: deps-resolver does not know which prefixes
  // are configured as named registries, and the cost is one extra metadata
  // fetch on re-resolution rather than a correctness bug.
  expect(replaceVersionInBareSpecifier('gh:^1.0.0', '1.1.0')).toBe('gh:^1.0.0')
  expect(replaceVersionInBareSpecifier('gh:@acme/foo@^1.0.0', '1.1.0')).toBe('gh:@acme/foo@^1.0.0')
  expect(replaceVersionInBareSpecifier('work:^1.0.0', '1.1.0')).toBe('work:^1.0.0')
  expect(replaceVersionInBareSpecifier('workspace:^1.0.0', '1.1.0')).toBe('workspace:^1.0.0')
  expect(replaceVersionInBareSpecifier('file:./pkg', '1.1.0')).toBe('file:./pkg')
  expect(replaceVersionInBareSpecifier('link:../pkg', '1.1.0')).toBe('link:../pkg')
  expect(replaceVersionInBareSpecifier('catalog:', '1.1.0')).toBe('catalog:')
  expect(replaceVersionInBareSpecifier('github:owner/repo', '1.1.0')).toBe('github:owner/repo')
  expect(replaceVersionInBareSpecifier('https://example.com/tarball.tgz', '1.1.0')).toBe('https://example.com/tarball.tgz')
})
