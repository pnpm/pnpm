import { normalizeBinObject } from '../lib/transform/bin.js'

test('string', () => {
  expect(normalizeBinObject('foo', 'bin.js')).toStrictEqual({ foo: 'bin.js' })
  expect(normalizeBinObject('@bar/foo', 'bin.js')).toStrictEqual({ foo: 'bin.js' })
})

test('object', () => {
  expect(normalizeBinObject('foo', {})).toStrictEqual({})
  expect(normalizeBinObject('foo', {
    foo: 'foo.js',
  })).toStrictEqual({
    foo: 'foo.js',
  })
  expect(normalizeBinObject('foo', {
    foo: 'foo.js',
    bar: 'bar.js',
  })).toStrictEqual({
    foo: 'foo.js',
    bar: 'bar.js',
  })
})
