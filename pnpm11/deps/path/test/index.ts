/// <reference path="../../../__typings__/index.d.ts"/>
import { expect, test } from '@jest/globals'
import {
  depPathToFilename,
  getPkgIdWithPatchHash,
  isAbsolute,
  isRuntimeDepPath,
  parse,
  parseRegistryQualifiedVersion,
  refToRelative,
  removeSuffix,
  tryGetPackageId,
} from '@pnpm/deps.path'
import type { DepPath } from '@pnpm/types'

test('isAbsolute()', () => {
  expect(isAbsolute('/foo/1.0.0')).toBeFalsy()
  expect(isAbsolute('registry.npmjs.org/foo/1.0.0')).toBeTruthy()
})

test('parse()', () => {
  /* eslint-disable @typescript-eslint/no-explicit-any */
  expect(() => parse(undefined as any)).toThrow(/got `undefined`/)
  expect(() => parse({} as any)).toThrow(/got `object`/)
  expect(() => parse(1 as any)).toThrow(/got `number`/)
  /* eslint-enable @typescript-eslint/no-explicit-any */
  expect(parse('foo@1.0.0')).toStrictEqual({
    name: 'foo',
    peerDepGraphHash: undefined,
    version: '1.0.0',
    patchHash: undefined,
  })

  expect(parse('@foo/bar@1.0.0')).toStrictEqual({
    name: '@foo/bar',
    peerDepGraphHash: undefined,
    version: '1.0.0',
    patchHash: undefined,
  })

  expect(parse('foo@1.0.0(@types/babel__core@7.1.14)')).toStrictEqual({
    name: 'foo',
    peerDepGraphHash: '(@types/babel__core@7.1.14)',
    version: '1.0.0',
    patchHash: undefined,
  })

  expect(parse('foo@1.0.0(@types/babel__core@7.1.14)(foo@1.0.0)')).toStrictEqual({
    name: 'foo',
    peerDepGraphHash: '(@types/babel__core@7.1.14)(foo@1.0.0)',
    version: '1.0.0',
    patchHash: undefined,
  })

  expect(parse('@(-.-)/foo@1.0.0(@types/babel__core@7.1.14)(foo@1.0.0)')).toStrictEqual({
    name: '@(-.-)/foo',
    peerDepGraphHash: '(@types/babel__core@7.1.14)(foo@1.0.0)',
    version: '1.0.0',
    patchHash: undefined,
  })

  expect(parse('tar-pkg@file:../tar-pkg-1.0.0.tgz')).toStrictEqual({
    name: 'tar-pkg',
    nonSemverVersion: 'file:../tar-pkg-1.0.0.tgz',
    peerDepGraphHash: undefined,
    patchHash: undefined,
  })

  expect(parse('foo@1.0.0(patch_hash=0000)(@types/babel__core@7.1.14)')).toStrictEqual({
    name: 'foo',
    peerDepGraphHash: '(@types/babel__core@7.1.14)',
    version: '1.0.0',
    patchHash: '(patch_hash=0000)',
  })
})

test('refToRelative()', () => {
  expect(refToRelative('1.3.0', '@most/multicast')).toBe('@most/multicast@1.3.0')
  expect(refToRelative('1.3.0', 'most')).toBe('most@1.3.0')
  expect(refToRelative('m@1.3.0', 'most')).toBe('m@1.3.0')
  expect(refToRelative('@most/multicast@1.3.0', 'most')).toBe('@most/multicast@1.3.0')
  expect(refToRelative('@most/multicast@1.3.0', '@most/multicast')).toBe('@most/multicast@1.3.0')
  expect(refToRelative('@most/multicast@1.3.0(@foo/bar@1.0.0)', '@most/multicast')).toBe('@most/multicast@1.3.0(@foo/bar@1.0.0)')
  expect(refToRelative('@most/multicast@1.3.0(@foo/bar@1.0.0)(@foo/qar@1.0.0)', '@most/multicast')).toBe('@most/multicast@1.3.0(@foo/bar@1.0.0)(@foo/qar@1.0.0)')
  // linked dependencies don't have a relative path
  expect(refToRelative('link:../foo', 'foo')).toBeNull()
  expect(refToRelative('file:../tarball.tgz', 'foo')).toBe('foo@file:../tarball.tgz')
  expect(refToRelative('1.3.0(@foo/bar@1.0.0)', '@qar/bar')).toBe('@qar/bar@1.3.0(@foo/bar@1.0.0)')
  expect(refToRelative('1.3.0(@foo/bar@1.0.0)(@foo/qar@1.0.0)', '@qar/bar')).toBe('@qar/bar@1.3.0(@foo/bar@1.0.0)(@foo/qar@1.0.0)')
})

test('depPathToFilename()', () => {
  expect(depPathToFilename('/foo@1.0.0', 120)).toBe('foo@1.0.0')
  expect(depPathToFilename('/@foo/bar@1.0.0', 120)).toBe('@foo+bar@1.0.0')
  expect(depPathToFilename('github.com/something/foo/0000?v=1', 120)).toBe('github.com+something+foo+0000+v=1')
  expect(depPathToFilename('\\//:*?"<>|', 120)).toBe('++++++++++')
  expect(depPathToFilename('/foo@1.0.0(react@16.0.0)(react-dom@16.0.0)', 120)).toBe('foo@1.0.0_react@16.0.0_react-dom@16.0.0')
  expect(depPathToFilename('/foo@1.0.0(react@16.0.0(react-dom@1.0.0))(react-dom@16.0.0)', 120)).toBe('foo@1.0.0_react@16.0.0_react-dom@1.0.0__react-dom@16.0.0')

  const filename = depPathToFilename('file:test/foo-1.0.0.tgz_foo@2.0.0', 120)
  expect(filename).toBe('file+test+foo-1.0.0.tgz_foo@2.0.0')
  expect(filename).not.toContain(':')

  expect(depPathToFilename('abcd/'.repeat(200), 120)).toBe('abcd+abcd+abcd+abcd+abcd+abcd+abcd+abcd+abcd+abcd+abcd+abcd+abcd+abcd+abcd+abcd+abcd+ab_e7c10c3598ebbc0ca640b6524c68e602') // cspell:disable-line
  expect(depPathToFilename('/JSONSteam@1.0.0', 120)).toBe('JSONSteam@1.0.0_533d3b11e9111b7a24f914844c021ddf') // cspell:disable-line

  expect(depPathToFilename('foo@git+https://github.com/something/foo#1234', 120)).toBe('foo@git+https+++github.com+something+foo+1234')
  expect(depPathToFilename('foo@https://codeload.github.com/something/foo/tar.gz/1234#path:packages/foo', 120)).toBe('foo@https+++codeload.github.com+something+foo+tar.gz+1234+path+packages+foo')
})

test('tryGetPackageId', () => {
  expect(tryGetPackageId('/foo@1.0.0(@types/babel__core@7.1.14)' as DepPath)).toBe('/foo@1.0.0')
  expect(tryGetPackageId('/foo@1.0.0(@types/babel__core@7.1.14(is-odd@1.0.0))' as DepPath)).toBe('/foo@1.0.0')
  expect(tryGetPackageId('/@(-.-)/foo@1.0.0(@types/babel__core@7.1.14)' as DepPath)).toBe('/@(-.-)/foo@1.0.0')
  expect(tryGetPackageId('foo@1.0.0(patch_hash=xxxx)(@types/babel__core@7.1.14)' as DepPath)).toBe('foo@1.0.0')
})

test('getPkgIdWithPatchHash', () => {
  // Runtime dependency
  expect(getPkgIdWithPatchHash('node@runtime:24.11.1' as DepPath)).toBe('node@runtime:24.11.1')

  // Regular packages
  expect(getPkgIdWithPatchHash('foo@1.0.0' as DepPath)).toBe('foo@1.0.0')

  // Packages with patch hash
  expect(getPkgIdWithPatchHash('foo@1.0.0(patch_hash=xxxx)' as DepPath)).toBe('foo@1.0.0(patch_hash=xxxx)')

  // Packages with peer dependencies (should remove peer dependencies)
  expect(getPkgIdWithPatchHash('foo@1.0.0(@types/babel__core@7.1.14)' as DepPath)).toBe('foo@1.0.0')

  // Packages with both patch hash and peer dependencies (should keep patch hash, remove peer dependencies)
  expect(getPkgIdWithPatchHash('foo@1.0.0(patch_hash=xxxx)(@types/babel__core@7.1.14)' as DepPath)).toBe('foo@1.0.0(patch_hash=xxxx)')

  // Scoped packages
  expect(getPkgIdWithPatchHash('@foo/bar@1.0.0' as DepPath)).toBe('@foo/bar@1.0.0')

  // Scoped packages with patch hash
  expect(getPkgIdWithPatchHash('@foo/bar@1.0.0(patch_hash=yyyy)' as DepPath)).toBe('@foo/bar@1.0.0(patch_hash=yyyy)')

  // Scoped packages with peer dependencies
  expect(getPkgIdWithPatchHash('@foo/bar@1.0.0(@types/node@18.0.0)' as DepPath)).toBe('@foo/bar@1.0.0')

  // Scoped packages with both patch hash and peer dependencies
  expect(getPkgIdWithPatchHash('@foo/bar@1.0.0(patch_hash=zzzz)(@types/node@18.0.0)' as DepPath)).toBe('@foo/bar@1.0.0(patch_hash=zzzz)')
})

test('isRuntimeDepPath', () => {
  expect(isRuntimeDepPath('node@runtime:20.1.0' as DepPath)).toBeTruthy()
  expect(isRuntimeDepPath('node@20.1.0' as DepPath)).toBeFalsy()
})

test('removeSuffix', () => {
  expect(removeSuffix('foo@1.0.0(patch_hash=0000)(@types/babel__core@7.1.14)')).toBe('foo@1.0.0')
})

test('parse() registry-qualified dep paths', () => {
  expect(parse('foo@work:1.0.0')).toStrictEqual({
    name: 'foo',
    peerDepGraphHash: undefined,
    version: '1.0.0',
    patchHash: undefined,
    registryName: 'work',
  })
  expect(parse('@acme/private@gh:2.1.0(react@18.0.0)')).toStrictEqual({
    name: '@acme/private',
    peerDepGraphHash: '(react@18.0.0)',
    version: '2.1.0',
    patchHash: undefined,
    registryName: 'gh',
  })
  // Reserved prefixes keep their existing non-semver meaning.
  expect(parse('foo@file:1.0.0').registryName).toBeUndefined()
  expect(parse('foo@file:1.0.0').nonSemverVersion).toBe('file:1.0.0')
  expect(parse('node@runtime:24.11.1').registryName).toBeUndefined()
  // A non-semver remainder is not registry-qualified.
  expect(parse('foo@work:not-semver').nonSemverVersion).toBe('work:not-semver')
})

test('parseRegistryQualifiedVersion()', () => {
  expect(parseRegistryQualifiedVersion('work:1.0.0')).toStrictEqual({ registryName: 'work', version: '1.0.0' })
  expect(parseRegistryQualifiedVersion('gh:2.1.0-beta.1')).toStrictEqual({ registryName: 'gh', version: '2.1.0-beta.1' })
  expect(parseRegistryQualifiedVersion('file:1.0.0')).toBeUndefined()
  expect(parseRegistryQualifiedVersion('runtime:24.0.0')).toBeUndefined()
  expect(parseRegistryQualifiedVersion('1.0.0')).toBeUndefined()
  expect(parseRegistryQualifiedVersion('work:^1.0.0')).toBeUndefined()
  expect(parseRegistryQualifiedVersion('9work:1.0.0')).toBeUndefined()
})

test('tryGetPackageId keeps registry-qualified ids whole', () => {
  expect(tryGetPackageId('foo@work:1.0.0(@types/babel__core@7.1.14)' as DepPath)).toBe('foo@work:1.0.0')
  expect(tryGetPackageId('@acme/private@gh:2.1.0' as DepPath)).toBe('@acme/private@gh:2.1.0')
})

test('refToRelative() reconstructs registry-qualified dep paths', () => {
  expect(refToRelative('work:1.0.0', 'foo')).toBe('foo@work:1.0.0')
  expect(refToRelative('work:1.0.0(react@18.0.0)', 'foo')).toBe('foo@work:1.0.0(react@18.0.0)')
  expect(refToRelative('@acme/private@gh:2.1.0', 'aliased')).toBe('@acme/private@gh:2.1.0')
})

test('depPathToFilename() escapes registry-qualified dep paths', () => {
  expect(depPathToFilename('foo@work:1.0.0', 120)).toBe('foo@work+1.0.0')
})
