import { getPatchInfo } from '../src/index'

test('getPatchInfo(undefined, ...) returns undefined', () => {
  expect(getPatchInfo(undefined, 'foo', '1.0.0')).toBeUndefined()
})

test('getPatchInfo(_, name, version) returns strict=true if name@version exists', () => {
  expect(getPatchInfo({
    'foo@1.0.0': {
      path: 'patches/foo@1.0.0.patch',
      hash: '00000000000000000000000000000000',
    },
  }, 'foo', '1.0.0')).toStrictEqual({
    file: {
      path: 'patches/foo@1.0.0.patch',
      hash: expect.any(String),
    },
    key: 'foo@1.0.0',
    strict: true,
  })
})

test('getPatchInfo(_, name, version) returns strict=false if name exists and name@version does not exist', () => {
  expect(getPatchInfo({
    foo: {
      path: 'patches/foo.patch',
      hash: '00000000000000000000000000000000',
    },
  }, 'foo', '1.0.0')).toStrictEqual({
    file: {
      path: 'patches/foo.patch',
      hash: expect.any(String),
    },
    key: 'foo',
    strict: false,
  })
})

test('getPatchInfo(_, name, version) prioritizes name@version over name if both exist', () => {
  expect(getPatchInfo({
    foo: {
      path: 'patches/foo.patch',
      hash: '00000000000000000000000000000000',
    },
    'foo@1.0.0': {
      path: 'patches/foo@1.0.0.patch',
      hash: '00000000000000000000000000000000',
    },
  }, 'foo', '1.0.0')).toStrictEqual({
    file: {
      path: 'patches/foo@1.0.0.patch',
      hash: expect.any(String),
    },
    key: 'foo@1.0.0',
    strict: true,
  })
})

test('getPatchInfo(_, name, version) does not access wrong name', () => {
  expect(getPatchInfo({
    'bar@1.0.0': {
      path: 'patches/bar@1.0.0.patch',
      hash: '00000000000000000000000000000000',
    },
  }, 'foo', '1.0.0')).toBeUndefined()
})

test('getPatchInfo(_, name, version) does not access wrong version', () => {
  expect(getPatchInfo({
    'foo@2.0.0': {
      path: 'patches/foo@2.0.0.patch',
      hash: '00000000000000000000000000000000',
    },
  }, 'foo', '1.0.0')).toBeUndefined()
})
