import { expect, it } from '@jest/globals'
import { createAllowBuildFunction, isBuildExplicitlyDisallowed } from '@pnpm/building.policy'
import type { DepPath } from '@pnpm/types'

function depPath (value: string): DepPath {
  return value as DepPath
}

it('should allowBuilds with true value', () => {
  const allowBuild = createAllowBuildFunction({
    allowBuilds: { foo: true, 'qar@1.0.0 || 2.0.0': true },
  })
  expect(typeof allowBuild).toBe('function')
  expect(allowBuild!(depPath('foo@1.0.0'))).toBe(true)
  expect(allowBuild!(depPath('bar@1.0.0'))).toBeUndefined()
  expect(allowBuild!(depPath('qar@1.1.0'))).toBeUndefined()
  expect(allowBuild!(depPath('qar@1.0.0'))).toBe(true)
  expect(allowBuild!(depPath('qar@2.0.0'))).toBe(true)
})

it('should allowBuilds with false value', () => {
  const allowBuild = createAllowBuildFunction({
    allowBuilds: { foo: false, bar: true },
  })
  expect(typeof allowBuild).toBe('function')
  expect(allowBuild!(depPath('foo@1.0.0'))).toBe(false)
  expect(allowBuild!(depPath('bar@1.0.0'))).toBe(true)
  expect(allowBuild!(depPath('baz@1.0.0'))).toBeUndefined()
})

it('should not allow patterns in allowBuilds', () => {
  const allowBuild = createAllowBuildFunction({
    allowBuilds: { 'is-*': true },
  })
  expect(typeof allowBuild).toBe('function')
  expect(allowBuild!(depPath('is-odd@1.0.0'))).toBeUndefined()
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
  expect(allowBuild!(depPath('foo@1.0.0'))).toBeTruthy()
  expect(allowBuild!(depPath('foo@git+https://github.com/org/foo.git#abc123'))).toBeTruthy()
})

it('should not apply package-name rules to artifact depPaths', () => {
  const allowBuild = createAllowBuildFunction({
    allowBuilds: { foo: true, bar: true },
  })
  expect(allowBuild!(depPath('foo@git+https://github.com/org/foo.git#abc123'))).toBeUndefined()
  expect(allowBuild!(depPath('bar@https://example.com/bar.tgz'))).toBeUndefined()
  expect(allowBuild!(depPath('foo@1.0.0'))).toBe(true)
})

it('should apply package-name rules to artifact depPaths when identity trust is overridden', () => {
  const allowBuild = createAllowBuildFunction({
    allowBuilds: { foo: true },
  })
  expect(allowBuild!(depPath('foo@git+https://github.com/org/foo.git#abc123'), {
    trustPackageIdentity: true,
  })).toBe(true)
  expect(allowBuild!(depPath('foo@1.0.0'), { trustPackageIdentity: false })).toBeUndefined()
})

it('should deny by package name regardless of identity trust', () => {
  const allowBuild = createAllowBuildFunction({
    allowBuilds: { foo: false },
  })
  expect(allowBuild!(depPath('foo@git+https://github.com/org/foo.git#abc123'))).toBe(false)
  expect(allowBuild!(depPath('foo@1.0.0'))).toBe(false)
})

it('should allow artifact depPaths by depPath key', () => {
  const allowBuild = createAllowBuildFunction({
    allowBuilds: {
      'foo@git+https://github.com/org/foo.git#abc123': true,
      'bar@https://codeload.github.com/org/bar/tar.gz/abc123': false,
      foo: true,
    },
  })
  expect(allowBuild!(depPath('foo@git+https://github.com/org/foo.git#abc123(react@19.0.0)'))).toBe(true)
  expect(allowBuild!(depPath('foo@git+https://github.com/attacker/foo.git#abc123'))).toBeUndefined()
  expect(allowBuild!(depPath('bar@https://codeload.github.com/org/bar/tar.gz/abc123'))).toBe(false)
})

it('should preserve patch hash in depPath allowBuild keys', () => {
  const allowBuild = createAllowBuildFunction({
    allowBuilds: {
      'foo@https://example.com/foo.tgz(patch_hash=aaaa)': true,
    },
  })
  expect(allowBuild!(depPath('foo@https://example.com/foo.tgz(patch_hash=aaaa)(react@19.0.0)'))).toBe(true)
  expect(allowBuild!(depPath('foo@https://example.com/foo.tgz(patch_hash=bbbb)(react@19.0.0)'))).toBeUndefined()
})

it('should allow untrusted package identity by source-only depPath', () => {
  const allowBuild = createAllowBuildFunction({
    allowBuilds: { 'github.com/org/foo/abc123': true },
  })
  expect(allowBuild!(depPath('github.com/org/foo/abc123(react@19.0.0)'))).toBe(true)
})

it('isBuildExplicitlyDisallowed() flags only builds the policy explicitly forbids', () => {
  const allowBuild = createAllowBuildFunction({
    allowBuilds: { foo: false, bar: true },
  })
  expect(isBuildExplicitlyDisallowed(depPath('foo@1.0.0'), allowBuild)).toBe(true)
  expect(isBuildExplicitlyDisallowed(depPath('bar@1.0.0'), allowBuild)).toBe(false)
  expect(isBuildExplicitlyDisallowed(depPath('baz@1.0.0'), allowBuild)).toBe(false)
})

it('isBuildExplicitlyDisallowed() returns false when no policy is set', () => {
  expect(isBuildExplicitlyDisallowed(depPath('foo@1.0.0'), undefined)).toBe(false)
})

it('isBuildExplicitlyDisallowed() returns false for unparsable depPaths', () => {
  const allowBuild = createAllowBuildFunction({ allowBuilds: { foo: false } })
  expect(isBuildExplicitlyDisallowed(depPath('not-a-valid-dep-path'), allowBuild)).toBe(false)
})
