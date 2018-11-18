import test = require('tape')
import {
  refToAbsolute,
  refToRelative,
  isAbsolute,
  parse,
  relative,
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

  t.throws(() => parse('/foo/bar'), /\/foo\/bar is an invalid relative dependency path/)

  t.end()
})

test('refToAbsolute()', t => {
  t.equal(refToAbsolute('1.0.0', 'foo', 'https://registry.npmjs.org/'), 'registry.npmjs.org/foo/1.0.0')
  t.equal(refToAbsolute('registry.npmjs.org/foo/1.0.0', 'foo', 'https://registry.npmjs.org/'), 'registry.npmjs.org/foo/1.0.0')
  t.equal(refToAbsolute('/foo/1.0.0', 'foo', 'https://registry.npmjs.org/'), 'registry.npmjs.org/foo/1.0.0')
  t.equal(refToAbsolute('link:../foo', 'foo', 'https://registry.npmjs.org/'), null, "linked dependencies don't have an absolute path")
  t.end()
})

test('refToRelative()', t => {
  t.equal(refToRelative('/@most/multicast/1.3.0/most@1.7.3', '@most/multicast'), '/@most/multicast/1.3.0/most@1.7.3')
  t.equal(refToRelative('link:../foo', 'foo'), null, "linked dependencies don't have a relative path")
  t.end()
})

test('relative()', t => {
  t.equal(relative('https://registry.npmjs.org/', 'registry.npmjs.org/foo/1.0.0'), '/foo/1.0.0')
  t.equal(relative('http://localhost:4873/', 'localhost+4873/foo/1.0.0'), '/foo/1.0.0')
  t.equal(relative('https://registry.npmjs.org/', 'registry.npmjs.org/foo/1.0.0/PeLdniYiO858gXNY39o5wISKyw'), '/foo/1.0.0/PeLdniYiO858gXNY39o5wISKyw')
  t.equal(relative('https://registry.npmjs.org/', 'registry.npmjs.org/foo/-/foo-1.0.0'), 'registry.npmjs.org/foo/-/foo-1.0.0', 'a tarball ID should remain absolute')
  t.end()
})
