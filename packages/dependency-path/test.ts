///<reference path="../../typings/index.d.ts"/>
import test = require('tape')
import {
  refToAbsolute,
  refToRelative,
  isAbsolute,
  parse,
  relative,
  resolve,
} from 'dependency-path'

test('isAbsolute()', t => {
  t.notOk(isAbsolute('/foo/1.0.0'))
  t.ok(isAbsolute('registry.npmjs.org/foo/1.0.0'))
  t.end()
})

test('parse()', t => {
  t.throws(() => parse(undefined as any), /got `undefined`/)
  t.throws(() => parse(1 as any), /got `number`/)

  t.deepEqual(parse('/foo/1.0.0'), {
    isAbsolute: false,
    name: 'foo',
    version: '1.0.0',
    host: undefined,
  })

  t.deepEqual(parse('/@foo/bar/1.0.0'), {
    isAbsolute: false,
    name: '@foo/bar',
    version: '1.0.0',
    host: undefined,
  })

  t.deepEqual(parse('registry.npmjs.org/foo/1.0.0'), {
    isAbsolute: true,
    name: 'foo',
    version: '1.0.0',
    host: 'registry.npmjs.org',
  })

  t.deepEqual(parse('registry.npmjs.org/@foo/bar/1.0.0'), {
    isAbsolute: true,
    name: '@foo/bar',
    version: '1.0.0',
    host: 'registry.npmjs.org',
  })

  t.deepEqual(parse('github.com/kevva/is-positive'), {
    isAbsolute: true,
    host: 'github.com',
  })

  t.deepEqual(parse('example.com/foo/1.0.0'), {
    isAbsolute: true,
    name: 'foo',
    version: '1.0.0',
    host: 'example.com',
  })

  t.deepEqual(parse('example.com/foo/1.0.0_bar@2.0.0'), {
    isAbsolute: true,
    name: 'foo',
    version: '1.0.0',
    host: 'example.com',
  })

  t.throws(() => parse('/foo/bar'), /\/foo\/bar is an invalid relative dependency path/)

  t.end()
})

test('refToAbsolute()', t => {
  const registries = {
    'default': 'https://registry.npmjs.org/',
    '@foo': 'http://foo.com/',
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
    'default': 'https://registry.npmjs.org/',
    '@foo': 'http://localhost:4873/',
  }
  t.equal(relative(registries, 'foo', 'registry.npmjs.org/foo/1.0.0'), '/foo/1.0.0')
  t.equal(relative(registries, '@foo/foo', 'localhost+4873/@foo/foo/1.0.0'), '/@foo/foo/1.0.0')
  t.equal(relative(registries, 'foo', 'registry.npmjs.org/foo/1.0.0/PeLdniYiO858gXNY39o5wISKyw'), '/foo/1.0.0/PeLdniYiO858gXNY39o5wISKyw')
  t.equal(relative(registries, 'foo', 'registry.npmjs.org/foo/-/foo-1.0.0'), 'registry.npmjs.org/foo/-/foo-1.0.0', 'a tarball ID should remain absolute')
  t.end()
})

test('resolve()', (t) => {
  const registries = {
    'default': 'htts://foo.com/',
    '@bar': 'https://bar.com/',
  }
  t.equal(resolve(registries, '/foo/1.0.0'), 'foo.com/foo/1.0.0')
  t.equal(resolve(registries, '/@bar/bar/1.0.0'), 'bar.com/@bar/bar/1.0.0')
  t.equal(resolve(registries, '/@qar/qar/1.0.0'), 'foo.com/@qar/qar/1.0.0')
  t.equal(resolve(registries, 'qar.com/foo/1.0.0'), 'qar.com/foo/1.0.0')
  t.end()
})
