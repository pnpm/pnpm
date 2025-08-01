/// <reference path="../../../__typings__/index.d.ts"/>
import {
  depPathToFilename,
  isAbsolute,
  parse,
  refToRelative,
  tryGetPackageId,
} from '@pnpm/dependency-path'
import { type DepPath } from '@pnpm/types'

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
  expect(refToRelative('1.3.0', '@most/multicast')).toEqual('@most/multicast@1.3.0')
  expect(refToRelative('1.3.0', 'most')).toEqual('most@1.3.0')
  expect(refToRelative('m@1.3.0', 'most')).toEqual('m@1.3.0')
  expect(refToRelative('@most/multicast@1.3.0', 'most')).toEqual('@most/multicast@1.3.0')
  expect(refToRelative('@most/multicast@1.3.0', '@most/multicast')).toEqual('@most/multicast@1.3.0')
  expect(refToRelative('@most/multicast@1.3.0(@foo/bar@1.0.0)', '@most/multicast')).toEqual('@most/multicast@1.3.0(@foo/bar@1.0.0)')
  expect(refToRelative('@most/multicast@1.3.0(@foo/bar@1.0.0)(@foo/qar@1.0.0)', '@most/multicast')).toEqual('@most/multicast@1.3.0(@foo/bar@1.0.0)(@foo/qar@1.0.0)')
  // linked dependencies don't have a relative path
  expect(refToRelative('link:../foo', 'foo')).toBeNull()
  expect(refToRelative('file:../tarball.tgz', 'foo')).toEqual('foo@file:../tarball.tgz')
  expect(refToRelative('1.3.0(@foo/bar@1.0.0)', '@qar/bar')).toEqual('@qar/bar@1.3.0(@foo/bar@1.0.0)')
  expect(refToRelative('1.3.0(@foo/bar@1.0.0)(@foo/qar@1.0.0)', '@qar/bar')).toEqual('@qar/bar@1.3.0(@foo/bar@1.0.0)(@foo/qar@1.0.0)')
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
  expect(tryGetPackageId('/foo@1.0.0(@types/babel__core@7.1.14)' as DepPath)).toEqual('/foo@1.0.0')
  expect(tryGetPackageId('/foo@1.0.0(@types/babel__core@7.1.14(is-odd@1.0.0))' as DepPath)).toEqual('/foo@1.0.0')
  expect(tryGetPackageId('/@(-.-)/foo@1.0.0(@types/babel__core@7.1.14)' as DepPath)).toEqual('/@(-.-)/foo@1.0.0')
  expect(tryGetPackageId('foo@1.0.0(patch_hash=xxxx)(@types/babel__core@7.1.14)' as DepPath)).toEqual('foo@1.0.0')
})
