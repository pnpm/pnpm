/// <reference path="../../../__typings__/index.d.ts"/>
import { expect, test } from '@jest/globals'
import {
  depPathToFilename,
  getPkgIdWithPatchHash,
  hasPatchHash,
  isAbsolute,
  isRuntimeDepPath,
  packageRootLinkTarget,
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
  expect(() => parse(undefined as unknown as string)).toThrow(/got `undefined`/)
  expect(() => parse({} as unknown as string)).toThrow(/got `object`/)
  expect(() => parse(1 as unknown as string)).toThrow(/got `number`/)
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

  expect(depPathToFilename('foo@git+https://github.com/something/foo', 120)).toBe('foo@git+https+++github.com+something+foo')
  expect(depPathToFilename('foo@git+https://github.com/something/foo#1234', 120)).toBe('foo@git+https+++github.com+something+foo+1234_b1a78add6ab51a177ff8780d0f64b41d')
  expect(depPathToFilename('foo@https://codeload.github.com/something/foo/tar.gz/1234#path:packages/foo', 120)).toBe('foo@https+++codeload.github.com+something+foo+tar.gz+1234+path+packages+foo_160b2e15ba002509e0f467796aa2dfd1')
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

test('depPathToFilename() escapes trailing dots and spaces', () => {
  expect(depPathToFilename('parent-pkg@file:..', 120)).toBe('parent-pkg@file+++_3cf6176c884f1541b42906b711973e2d')
  expect(depPathToFilename('pkg@file:.', 120)).toBe('pkg@file++_f8a4bd4027dd0dda71549ddff4eb2bbb')
  expect(depPathToFilename('pkg@file:../dir ', 120)).toBe('pkg@file+..+dir+_58ccd8dce4811ac686d72920d5724090')
  expect(depPathToFilename('foo@1.0.0(pkg@file:..)', 120)).toBe('foo@1.0.0_pkg@file+++_532b5e0801878347427004a15da818ef')
  expect(depPathToFilename('parent-pkg@file:++', 120)).toBe('parent-pkg@file+++_533e2d775a8ebec1166dcc5f3df4de30')
  expect(depPathToFilename('pkg@file:../project-2', 120)).toBe('pkg@file+..+project-2')
  expect(depPathToFilename('Parent-pkg@file:..', 120)).not.toBe(depPathToFilename('Parent-pkg@file:++', 120))
})

test('depPathToFilename() escapes registry-qualified dep paths', () => {
  expect(depPathToFilename('foo@work:1.0.0', 120)).toBe('foo@work+1.0.0')
})

test('hasPatchHash()', () => {
  expect(hasPatchHash('foo@1.0.0(patch_hash=abc)')).toBe(true)
  expect(hasPatchHash('foo@1.0.0(patch_hash=abc)(bar@2.0.0)')).toBe(true)
  expect(hasPatchHash('foo@1.0.0(bar@2.0.0(patch_hash=abc))')).toBe(false)
  expect(hasPatchHash('foo@1.0.0')).toBe(false)
})

test('packageRootLinkTarget() accepts only plain paths inside the package', () => {
  expect(packageRootLinkTarget('link:<root>/typings/css-tree')).toBe('typings/css-tree')
  for (const reference of [
    'link:<root>/',
    'link:<root>/../outside',
    'link:<root>/a/../../outside',
    'link:<root>/a/./b',
    'link:<root>/a//b',
    'link:<root>/C:/Users/Public',
    'link:<root>/a\\..\\..\\outside',
    'link:packages/c',
  ]) {
    expect(packageRootLinkTarget(reference)).toBeUndefined()
  }
})

test('depPathToFilename() hashes URLs whose escaping is ambiguous', () => {
  const base = 'pkg@https://registry.example.com/objects/trusted'
  expect(depPathToFilename(`${base}/package.tgz`, 120)).toBe('pkg@https+++registry.example.com+objects+trusted+package.tgz')
  expect(depPathToFilename(`${base}+package.tgz`, 120)).toBe('pkg@https+++registry.example.com+objects+trusted+package.tgz_5b1382866f68e516badc06db3d605b81')
  expect(depPathToFilename(`${base}:package.tgz`, 120)).toBe('pkg@https+++registry.example.com+objects+trusted+package.tgz_1536617e57923d7a634ea5063d52d423')
  expect(depPathToFilename(`${base}?package.tgz`, 120)).toBe('pkg@https+++registry.example.com+objects+trusted+package.tgz_56ed6c679663e41c52a329ea7ffc783f')
  expect(depPathToFilename(`${base}#package.tgz`, 120)).toBe('pkg@https+++registry.example.com+objects+trusted+package.tgz_30421c24d7ece624b64805feea2d87f4')
  expect(depPathToFilename(`${base}\\package.tgz`, 120)).toBe('pkg@https+++registry.example.com+objects+trusted+package.tgz_c28afc38506f4f342864ce00ee3373c8')
  expect(depPathToFilename(`${base}+package.tgz`, 40)).toBe('pkg@htt_5b1382866f68e516badc06db3d605b81')
  expect(depPathToFilename(`${base}/package.tgz`, 40)).toBe('pkg@htt_db0178b93a3dc73ecc84ba68ab60a4ab')
})

test('depPathToFilename() keeps registry versions with build metadata without a hash suffix', () => {
  expect(depPathToFilename('foo@1.0.0+build.5', 120)).toBe('foo@1.0.0+build.5')
  expect(depPathToFilename('esbuild@0.0.0-dev+abc(foo@1.0.0)', 120)).toBe('esbuild@0.0.0-dev+abc_foo@1.0.0')
  expect(depPathToFilename('foo@work:1.0.0+build', 120)).toBe('foo@work+1.0.0+build')
})
