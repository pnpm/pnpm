/// <reference path="../../../typings/index.d.ts"/>
import {
  isAbsolute,
  parse,
  refToAbsolute,
  refToRelative,
  relative,
  resolve,
} from 'dependency-path'
import test = require('tape')

test('isAbsolute()', t => {
  t.notOk(isAbsolute('/foo/1.0.0'))
  t.ok(isAbsolute('registry.npmjs.org/foo/1.0.0'))
  t.end()
})

test('parse()', t => {
  /* eslint-disable @typescript-eslint/no-explicit-any */
  t.throws(() => parse(undefined as any), /got `undefined`/)
  t.throws(() => parse(null as any), /got `null`/)
  t.throws(() => parse({} as any), /got `object`/)
  t.throws(() => parse(1 as any), /got `number`/)
  /* eslint-enable @typescript-eslint/no-explicit-any */

  t.deepEqual(parse('/foo/1.0.0'), {
    host: undefined,
    isAbsolute: false,
    name: 'foo',
    peersSuffix: undefined,
    version: '1.0.0',
  })

  t.deepEqual(parse('/@foo/bar/1.0.0'), {
    host: undefined,
    isAbsolute: false,
    name: '@foo/bar',
    peersSuffix: undefined,
    version: '1.0.0',
  })

  t.deepEqual(parse('registry.npmjs.org/foo/1.0.0'), {
    host: 'registry.npmjs.org',
    isAbsolute: true,
    name: 'foo',
    peersSuffix: undefined,
    version: '1.0.0',
  })

  t.deepEqual(parse('registry.npmjs.org/@foo/bar/1.0.0'), {
    host: 'registry.npmjs.org',
    isAbsolute: true,
    name: '@foo/bar',
    peersSuffix: undefined,
    version: '1.0.0',
  })

  t.deepEqual(parse('github.com/kevva/is-positive'), {
    host: 'github.com',
    isAbsolute: true,
  })

  t.deepEqual(parse('example.com/foo/1.0.0'), {
    host: 'example.com',
    isAbsolute: true,
    name: 'foo',
    peersSuffix: undefined,
    version: '1.0.0',
  })

  t.deepEqual(parse('example.com/foo/1.0.0_bar@2.0.0'), {
    host: 'example.com',
    isAbsolute: true,
    name: 'foo',
    peersSuffix: 'bar@2.0.0',
    version: '1.0.0',
  })

  t.throws(() => parse('/foo/bar'), /\/foo\/bar is an invalid relative dependency path/)

  t.end()
})

test('refToAbsolute()', t => {
  const registries = {
    '@foo': 'http://foo.com/',
    default: 'https://registry.npmjs.org/',
  }
  t.equal(refToAbsolute('1.0.0', 'foo', registries), 'registry.npmjs.org/foo/1.0.0')
  t.equal(refToAbsolute('1.0.0', '@foo/foo', registries), 'foo.com/@foo/foo/1.0.0')
  t.equal(refToAbsolute('registry.npmjs.org/foo/1.0.0', 'foo', registries), 'registry.npmjs.org/foo/1.0.0')
  t.equal(refToAbsolute('/foo/1.0.0', 'foo', registries), 'registry.npmjs.org/foo/1.0.0')
  t.equal(refToAbsolute('/@foo/foo/1.0.0', '@foo/foo', registries), 'foo.com/@foo/foo/1.0.0')
  t.equal(refToAbsolute('link:../foo', 'foo', registries), null, "linked dependencies don't have an absolute path")
  t.end()
})

test('refToRelative()', t => {
  t.equal(refToRelative('/@most/multicast/1.3.0/most@1.7.3', '@most/multicast'), '/@most/multicast/1.3.0/most@1.7.3')
  t.equal(refToRelative('link:../foo', 'foo'), null, "linked dependencies don't have a relative path")
  t.equal(refToRelative('file:../tarball.tgz', 'foo'), 'file:../tarball.tgz')
  t.end()
})

test('relative()', t => {
  const registries = {
    '@foo': 'http://localhost:4873/',
    default: 'https://registry.npmjs.org/',
  }
  t.equal(relative(registries, 'foo', 'registry.npmjs.org/foo/1.0.0'), '/foo/1.0.0')
  t.equal(relative(registries, '@foo/foo', 'localhost+4873/@foo/foo/1.0.0'), '/@foo/foo/1.0.0')
  t.equal(relative(registries, 'foo', 'registry.npmjs.org/foo/1.0.0/PeLdniYiO858gXNY39o5wISKyw'), '/foo/1.0.0/PeLdniYiO858gXNY39o5wISKyw')
  t.end()
})

test('resolve()', (t) => {
  const registries = {
    '@bar': 'https://bar.com/',
    default: 'https://foo.com/',
  }
  t.equal(resolve(registries, '/foo/1.0.0'), 'foo.com/foo/1.0.0')
  t.equal(resolve(registries, '/@bar/bar/1.0.0'), 'bar.com/@bar/bar/1.0.0')
  t.equal(resolve(registries, '/@qar/qar/1.0.0'), 'foo.com/@qar/qar/1.0.0')
  t.equal(resolve(registries, 'qar.com/foo/1.0.0'), 'qar.com/foo/1.0.0')
  t.end()
})
