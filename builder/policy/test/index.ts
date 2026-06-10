import path from 'path'
import { allowBuildKeyFromIgnoredBuild, createAllowBuildFunction } from '@pnpm/builder.policy'
import { type DepPath } from '@pnpm/types'

function depPath (value: string): DepPath {
  return value as DepPath
}

it('should neverBuiltDependencies', () => {
  const allowBuild = createAllowBuildFunction({
    neverBuiltDependencies: ['foo'],
  })
  expect(typeof allowBuild).toBe('function')
  expect(allowBuild!(depPath('foo@1.0.0'))).toBeFalsy()
  expect(allowBuild!(depPath('bar@1.0.0'))).toBeTruthy()
})

it('should deny neverBuiltDependencies by name even for artifact depPaths', () => {
  const allowBuild = createAllowBuildFunction({
    neverBuiltDependencies: ['foo'],
  })
  expect(allowBuild!(depPath('foo@git+https://github.com/org/foo.git#abc123'))).toBeFalsy()
  expect(allowBuild!(depPath('bar@git+https://github.com/org/bar.git#abc123'))).toBeTruthy()
})

it('should onlyBuiltDependencies', () => {
  const allowBuild = createAllowBuildFunction({
    onlyBuiltDependencies: ['foo', 'qar@1.0.0 || 2.0.0'],
  })
  expect(typeof allowBuild).toBe('function')
  expect(allowBuild!(depPath('foo@1.0.0'))).toBeTruthy()
  expect(allowBuild!(depPath('bar@1.0.0'))).toBeFalsy()
  expect(allowBuild!(depPath('qar@1.1.0'))).toBeFalsy()
  expect(allowBuild!(depPath('qar@1.0.0'))).toBeTruthy()
  expect(allowBuild!(depPath('qar@2.0.0'))).toBeTruthy()
})

it('should not allow patterns in onlyBuiltDependencies', () => {
  const allowBuild = createAllowBuildFunction({
    onlyBuiltDependencies: ['is-*'],
  })
  expect(typeof allowBuild).toBe('function')
  expect(allowBuild!(depPath('is-odd@1.0.0'))).toBeFalsy()
})

it('should onlyBuiltDependencies set via a file', () => {
  const allowBuild = createAllowBuildFunction({
    onlyBuiltDependenciesFile: path.join(__dirname, 'onlyBuild.json'),
  })
  expect(typeof allowBuild).toBe('function')
  expect(allowBuild!(depPath('zoo@1.0.0'))).toBeTruthy()
  expect(allowBuild!(depPath('qar@1.0.0'))).toBeTruthy()
  expect(allowBuild!(depPath('bar@1.0.0'))).toBeFalsy()
})

it('should onlyBuiltDependencies set via a file and config', () => {
  const allowBuild = createAllowBuildFunction({
    onlyBuiltDependencies: ['bar'],
    onlyBuiltDependenciesFile: path.join(__dirname, 'onlyBuild.json'),
  })
  expect(typeof allowBuild).toBe('function')
  expect(allowBuild!(depPath('zoo@1.0.0'))).toBeTruthy()
  expect(allowBuild!(depPath('qar@1.0.0'))).toBeTruthy()
  expect(allowBuild!(depPath('bar@1.0.0'))).toBeTruthy()
  expect(allowBuild!(depPath('esbuild@1.0.0'))).toBeFalsy()
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
    onlyBuiltDependencies: ['foo', 'bar'],
  })
  expect(allowBuild!(depPath('foo@git+https://github.com/org/foo.git#abc123'))).toBeFalsy()
  expect(allowBuild!(depPath('bar@https://example.com/bar.tgz'))).toBeFalsy()
  expect(allowBuild!(depPath('foo@1.0.0'))).toBeTruthy()
})

it('should allow artifact depPaths by depPath key', () => {
  const allowBuild = createAllowBuildFunction({
    onlyBuiltDependencies: [
      'foo@git+https://github.com/org/foo.git#abc123',
      'foo',
    ],
  })
  expect(allowBuild!(depPath('foo@git+https://github.com/org/foo.git#abc123(react@19.0.0)'))).toBeTruthy()
  expect(allowBuild!(depPath('foo@git+https://github.com/attacker/foo.git#abc123'))).toBeFalsy()
})

it('should preserve patch hash in depPath onlyBuiltDependencies keys', () => {
  const allowBuild = createAllowBuildFunction({
    onlyBuiltDependencies: [
      'foo@https://example.com/foo.tgz(patch_hash=aaaa)',
    ],
  })
  expect(allowBuild!(depPath('foo@https://example.com/foo.tgz(patch_hash=aaaa)(react@19.0.0)'))).toBeTruthy()
  expect(allowBuild!(depPath('foo@https://example.com/foo.tgz(patch_hash=bbbb)(react@19.0.0)'))).toBeFalsy()
})

it('should allow untrusted package identity by source-only depPath', () => {
  const allowBuild = createAllowBuildFunction({
    onlyBuiltDependencies: ['github.com/org/foo/abc123'],
  })
  expect(allowBuild!(depPath('github.com/org/foo/abc123(react@19.0.0)'))).toBeTruthy()
})

it('allowBuildKeyFromIgnoredBuild() returns the name for registry packages and the depPath for artifacts', () => {
  expect(allowBuildKeyFromIgnoredBuild(depPath('foo@1.0.0'))).toBe('foo')
  expect(allowBuildKeyFromIgnoredBuild(depPath('foo@1.0.0(react@19.0.0)'))).toBe('foo')
  expect(allowBuildKeyFromIgnoredBuild(depPath('foo@git+https://github.com/org/foo.git#abc123'))).toBe('foo@git+https://github.com/org/foo.git#abc123')
  expect(allowBuildKeyFromIgnoredBuild(depPath('bar@https://example.com/bar.tgz(react@19.0.0)'))).toBe('bar@https://example.com/bar.tgz')
})
