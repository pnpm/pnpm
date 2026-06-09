import { expect, it } from '@jest/globals'
import { createAllowBuildContext, createAllowBuildFunction, isBuildExplicitlyDisallowed } from '@pnpm/building.policy'
import type { DepPath } from '@pnpm/types'

it('should allowBuilds with true value', () => {
  const allowBuild = createAllowBuildFunction({
    allowBuilds: { foo: true, 'qar@1.0.0 || 2.0.0': true },
  })
  expect(typeof allowBuild).toBe('function')
  expect(allowBuild!('foo', '1.0.0')).toBe(true)
  expect(allowBuild!('bar', '1.0.0')).toBeUndefined()
  expect(allowBuild!('qar', '1.1.0')).toBeUndefined()
  expect(allowBuild!('qar', '1.0.0')).toBe(true)
  expect(allowBuild!('qar', '2.0.0')).toBe(true)
})

it('should allowBuilds with false value', () => {
  const allowBuild = createAllowBuildFunction({
    allowBuilds: { foo: false, bar: true },
  })
  expect(typeof allowBuild).toBe('function')
  expect(allowBuild!('foo', '1.0.0')).toBe(false)
  expect(allowBuild!('bar', '1.0.0')).toBe(true)
  expect(allowBuild!('baz', '1.0.0')).toBeUndefined()
})

it('should not allow patterns in allowBuilds', () => {
  const allowBuild = createAllowBuildFunction({
    allowBuilds: { 'is-*': true },
  })
  expect(typeof allowBuild).toBe('function')
  expect(allowBuild!('is-odd', '1.0.0')).toBeUndefined()
})

it('should reject invalid package versions in allowBuilds', () => {
  expect(() => createAllowBuildFunction({
    allowBuilds: { 'foo@not-a-version': true },
  })).toThrow(expect.objectContaining({ code: 'ERR_PNPM_INVALID_VERSION_UNION' }))
})

it('should return undefined if no policy is set', () => {
  expect(createAllowBuildFunction({})).toBeUndefined()
})

it('should allow everything when dangerouslyAllowAllBuilds is true', () => {
  const allowBuild = createAllowBuildFunction({
    dangerouslyAllowAllBuilds: true,
  })
  expect(typeof allowBuild).toBe('function')
  expect(allowBuild!('foo', '1.0.0')).toBeTruthy()
  expect(allowBuild!('foo', '1.0.0', { trustPackageIdentity: false })).toBeTruthy()
})

it('should require trusted package identity for allowBuilds with true value', () => {
  const allowBuild = createAllowBuildFunction({
    allowBuilds: { foo: true, bar: true },
  })
  expect(allowBuild!('foo', '1.0.0', { trustPackageIdentity: false })).toBeUndefined()
  expect(allowBuild!('bar', '1.0.0', { trustPackageIdentity: false })).toBeUndefined()
  expect(allowBuild!('foo', '1.0.0', { trustPackageIdentity: true })).toBe(true)
})

it('should allow untrusted package identity by depPath', () => {
  const allowBuild = createAllowBuildFunction({
    allowBuilds: {
      'foo@git+https://github.com/org/foo.git#abc123': true,
      'bar@https://codeload.github.com/org/bar/tar.gz/abc123': false,
      foo: true,
    },
  })
  expect(allowBuild!('foo', '1.0.0', {
    depPath: 'foo@git+https://github.com/org/foo.git#abc123(react@19.0.0)',
    trustPackageIdentity: false,
  })).toBe(true)
  expect(allowBuild!('foo', '1.0.0', {
    depPath: 'foo@git+https://github.com/attacker/foo.git#abc123',
    trustPackageIdentity: false,
  })).toBeUndefined()
  expect(allowBuild!('bar', '1.0.0', {
    depPath: 'bar@https://codeload.github.com/org/bar/tar.gz/abc123',
    trustPackageIdentity: false,
  })).toBe(false)
})

it('should preserve patch hash in depPath allowBuild keys', () => {
  const allowBuild = createAllowBuildFunction({
    allowBuilds: {
      'foo@https://example.com/foo.tgz(patch_hash=aaaa)': true,
    },
  })
  expect(allowBuild!('foo', '1.0.0', {
    depPath: 'foo@https://example.com/foo.tgz(patch_hash=aaaa)(react@19.0.0)',
    trustPackageIdentity: false,
  })).toBe(true)
  expect(allowBuild!('foo', '1.0.0', {
    depPath: 'foo@https://example.com/foo.tgz(patch_hash=bbbb)(react@19.0.0)',
    trustPackageIdentity: false,
  })).toBeUndefined()
  expect(createAllowBuildContext({
    depPath: 'foo@https://example.com/foo.tgz(patch_hash=aaaa)(react@19.0.0)',
  }).depPath).toBe('foo@https://example.com/foo.tgz(patch_hash=aaaa)')
})

it('should allow untrusted package identity by source-only depPath', () => {
  const allowBuild = createAllowBuildFunction({
    allowBuilds: { 'github.com/org/foo/abc123': true },
  })
  expect(allowBuild!('foo', '1.0.0', {
    depPath: 'github.com/org/foo/abc123(react@19.0.0)' as DepPath,
    trustPackageIdentity: false,
  })).toBe(true)
})

it('should create untrusted allowBuild context for artifact identities', () => {
  expect(createAllowBuildContext({
    depPath: 'foo@https://example.com/foo.tgz',
  }).trustPackageIdentity).toBe(false)
  expect(createAllowBuildContext({
    depPath: 'foo@https://example.com/foo.tgz(react@19.0.0)',
  }).depPath).toBe('foo@https://example.com/foo.tgz')
  expect(createAllowBuildContext({
    depPath: 'foo@1.0.0',
    resolution: { type: 'git' },
  }).trustPackageIdentity).toBe(false)
  expect(createAllowBuildContext({
    depPath: 'foo@1.0.0',
    resolution: { gitHosted: true },
  }).trustPackageIdentity).toBe(false)
  expect(createAllowBuildContext({
    depPath: 'foo@1.0.0',
    resolvedVia: 'git-repository',
  }).trustPackageIdentity).toBe(false)
  expect(createAllowBuildContext({
    depPath: 'foo@1.0.0',
  }).trustPackageIdentity).toBe(true)
})

it('should create trusted allowBuild context for registry, workspace, and registry tarball metadata', () => {
  expect(createAllowBuildContext({
    depPath: 'foo@1.0.0',
    resolvedVia: 'npm-registry',
  }).trustPackageIdentity).toBe(true)
  expect(createAllowBuildContext({
    depPath: 'foo@1.0.0',
    resolvedVia: 'workspace',
  }).trustPackageIdentity).toBe(true)
  expect(createAllowBuildContext({
    depPath: 'foo@1.0.0',
    resolution: {
      integrity: 'sha512-abc',
      tarball: 'https://registry.npmjs.org/foo/-/foo-1.0.0.tgz',
    },
  }).trustPackageIdentity).toBe(true)
})

it('isBuildExplicitlyDisallowed() flags only builds the policy explicitly forbids', () => {
  const allowBuild = createAllowBuildFunction({
    allowBuilds: { foo: false, bar: true },
  })
  expect(isBuildExplicitlyDisallowed('foo@1.0.0' as DepPath, allowBuild)).toBe(true)
  expect(isBuildExplicitlyDisallowed('bar@1.0.0' as DepPath, allowBuild)).toBe(false)
  expect(isBuildExplicitlyDisallowed('baz@1.0.0' as DepPath, allowBuild)).toBe(false)
})

it('isBuildExplicitlyDisallowed() returns false when no policy is set', () => {
  expect(isBuildExplicitlyDisallowed('foo@1.0.0' as DepPath, undefined)).toBe(false)
})

it('isBuildExplicitlyDisallowed() returns false for unparsable depPaths', () => {
  const allowBuild = createAllowBuildFunction({ allowBuilds: { foo: false } })
  expect(isBuildExplicitlyDisallowed('not-a-valid-dep-path' as DepPath, allowBuild)).toBe(false)
})
