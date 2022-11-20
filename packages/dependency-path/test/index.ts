/// <reference path="../../../__typings__/index.d.ts"/>
import {
  depPathToFilename,
  isAbsolute,
  parse,
  refToAbsolute,
  refToRelative,
  relative,
  resolve,
  tryGetPackageId,
} from 'dependency-path'

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
  expect(parse('/foo/1.0.0')).toStrictEqual({
    host: undefined,
    isAbsolute: false,
    name: 'foo',
    peersSuffix: undefined,
    version: '1.0.0',
  })

  expect(parse('/@foo/bar/1.0.0')).toStrictEqual({
    host: undefined,
    isAbsolute: false,
    name: '@foo/bar',
    peersSuffix: undefined,
    version: '1.0.0',
  })

  expect(parse('registry.npmjs.org/foo/1.0.0')).toStrictEqual({
    host: 'registry.npmjs.org',
    isAbsolute: true,
    name: 'foo',
    peersSuffix: undefined,
    version: '1.0.0',
  })

  expect(parse('registry.npmjs.org/@foo/bar/1.0.0')).toStrictEqual({
    host: 'registry.npmjs.org',
    isAbsolute: true,
    name: '@foo/bar',
    peersSuffix: undefined,
    version: '1.0.0',
  })

  expect(parse('github.com/kevva/is-positive')).toStrictEqual({
    host: 'github.com',
    isAbsolute: true,
  })

  expect(parse('example.com/foo/1.0.0')).toStrictEqual({
    host: 'example.com',
    isAbsolute: true,
    name: 'foo',
    peersSuffix: undefined,
    version: '1.0.0',
  })

  expect(parse('example.com/foo/1.0.0_bar@2.0.0')).toStrictEqual({
    host: 'example.com',
    isAbsolute: true,
    name: 'foo',
    peersSuffix: 'bar@2.0.0',
    version: '1.0.0',
  })

  expect(parse('/foo/1.0.0_@types+babel__core@7.1.14')).toStrictEqual({
    host: undefined,
    isAbsolute: false,
    name: 'foo',
    peersSuffix: '@types+babel__core@7.1.14',
    version: '1.0.0',
  })

  expect(() => parse('/foo/bar')).toThrow(/\/foo\/bar is an invalid relative dependency path/)
})

test('refToAbsolute()', () => {
  const registries = {
    '@foo': 'http://foo.com/',
    default: 'https://registry.npmjs.org/',
  }
  expect(refToAbsolute('1.0.0', 'foo', registries)).toEqual('registry.npmjs.org/foo/1.0.0')
  expect(refToAbsolute('1.0.0', '@foo/foo', registries)).toEqual('foo.com/@foo/foo/1.0.0')
  expect(refToAbsolute('registry.npmjs.org/foo/1.0.0', 'foo', registries)).toEqual('registry.npmjs.org/foo/1.0.0')
  expect(refToAbsolute('/foo/1.0.0', 'foo', registries)).toEqual('registry.npmjs.org/foo/1.0.0')
  expect(refToAbsolute('/@foo/foo/1.0.0', '@foo/foo', registries)).toEqual('foo.com/@foo/foo/1.0.0')
  // linked dependencies don't have an absolute path
  expect(refToAbsolute('link:../foo', 'foo', registries)).toBeNull()
})

test('refToRelative()', () => {
  expect(refToRelative('/@most/multicast/1.3.0/most@1.7.3', '@most/multicast')).toEqual('/@most/multicast/1.3.0/most@1.7.3')
  // linked dependencies don't have a relative path
  expect(refToRelative('link:../foo', 'foo')).toBeNull()
  expect(refToRelative('file:../tarball.tgz', 'foo')).toEqual('file:../tarball.tgz')
})

test('relative()', () => {
  const registries = {
    '@foo': 'http://localhost:4873/',
    default: 'https://registry.npmjs.org/',
  }
  expect(relative(registries, 'foo', 'registry.npmjs.org/foo/1.0.0')).toEqual('/foo/1.0.0')
  expect(relative(registries, '@foo/foo', 'localhost+4873/@foo/foo/1.0.0')).toEqual('/@foo/foo/1.0.0')
  expect(relative(registries, 'foo', 'registry.npmjs.org/foo/1.0.0/PeLdniYiO858gXNY39o5wISKyw')).toEqual('/foo/1.0.0/PeLdniYiO858gXNY39o5wISKyw')
})

test('resolve()', () => {
  const registries = {
    '@bar': 'https://bar.com/',
    default: 'https://foo.com/',
  }
  expect(resolve(registries, '/foo/1.0.0')).toEqual('foo.com/foo/1.0.0')
  expect(resolve(registries, '/@bar/bar/1.0.0')).toEqual('bar.com/@bar/bar/1.0.0')
  expect(resolve(registries, '/@qar/qar/1.0.0')).toEqual('foo.com/@qar/qar/1.0.0')
  expect(resolve(registries, 'qar.com/foo/1.0.0')).toEqual('qar.com/foo/1.0.0')
})

test('depPathToFilename()', () => {
  expect(depPathToFilename('/foo/1.0.0')).toBe('foo@1.0.0')
  expect(depPathToFilename('/@foo/bar/1.0.0')).toBe('@foo+bar@1.0.0')
  expect(depPathToFilename('github.com/something/foo/0000?v=1')).toBe('github.com+something+foo@0000+v=1')
  expect(depPathToFilename('\\//:*?"<>|')).toBe('++@+++++++')

  const filename = depPathToFilename('file:test/foo-1.0.0.tgz_foo@2.0.0')
  expect(filename).toBe('file+test+foo-1.0.0.tgz_foo@2.0.0')
  expect(filename).not.toContain(':')

  expect(depPathToFilename('abcd/'.repeat(200))).toBe('abcd+abcd+abcd+abcd+abcd+abcd+abcd+abcd+abcd+abcd+_e5jega7r3xmarw3h6f277a3any')
  expect(depPathToFilename('/JSONSteam/1.0.0')).toBe('JSONSteam@1.0.0_jmswpk4sf667aelr6wp2xd3p54')
})

test('tryGetPackageId', () => {
  expect(tryGetPackageId({ default: 'https://registry.npmjs.org/' }, '/foo/1.0.0_@types+babel__core@7.1.14')).toEqual('registry.npmjs.org/foo/1.0.0')
})
