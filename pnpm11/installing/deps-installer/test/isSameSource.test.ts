import { expect, test } from '@jest/globals'

import { isSameSource } from '../src/isSameSource.js'

test('isSameSource checks whether specifiers point to the same source', () => {
  expect(isSameSource('github:foo-org/skills#main', 'github:foo-org/skills#v1', 'skills')).toBe(true)
  expect(isSameSource('github:foo-org/skills', 'github:vercel-labs/skills', 'skills')).toBe(false)
  expect(isSameSource('^1.0.0', '^2.0.0', 'express')).toBe(true)
  expect(isSameSource('^1.0.0', 'github:vercel-labs/skills', 'skills')).toBe(false)
  expect(isSameSource('npm:foo@1.0.0', 'npm:foo@2.0.0', 'my-foo')).toBe(true)
  expect(isSameSource('npm:foo@1.0.0', 'npm:bar@1.0.0', 'my-foo')).toBe(false)
  expect(isSameSource('npm:@scope/foo@1.0.0', 'npm:@scope/foo@2.0.0', 'my-foo')).toBe(true)
  expect(isSameSource('npm:@scope/foo@1.0.0', 'npm:@scope/bar@1.0.0', 'my-foo')).toBe(false)
  expect(isSameSource('git+https://git.example.com/repo.git#main', 'git+https://git.example.com/repo.git#v1', 'repo')).toBe(true)
  expect(isSameSource('file:../foo', 'file:../bar', 'foo')).toBe(false)
  expect(isSameSource('file:../foo', 'file:../foo', 'foo')).toBe(true)
  expect(isSameSource('workspace:*', 'workspace:^1.0.0', 'my-pkg')).toBe(true)
  expect(isSameSource('catalog:default', 'catalog:default', 'my-pkg')).toBe(true)
  expect(isSameSource('catalog:foo', 'catalog:bar', 'my-pkg')).toBe(false)
  expect(isSameSource('https://example.com/a.tgz#1', 'https://example.com/a.tgz#2', 'my-pkg')).toBe(true)
  expect(isSameSource('https://example.com/a.tgz', 'https://example.com/b.tgz', 'my-pkg')).toBe(false)
})
