import { getPatchInfo, groupPatchedDependencies } from '../src/index.js'

const _getPatchInfo = (patchedDependencies: Record<string, string>, name: string, version: string) =>
  getPatchInfo(groupPatchedDependencies(patchedDependencies), name, version)

test('getPatchInfo(undefined, ...) returns undefined', () => {
  expect(getPatchInfo(undefined, 'foo', '1.0.0')).toBeUndefined()
})

test('getPatchInfo(_, name, version) if name@version exists', () => {
  expect(_getPatchInfo({
    'foo@1.0.0': '00000000000000000000000000000000',
  }, 'foo', '1.0.0')).toStrictEqual({
    hash: expect.any(String),
    key: 'foo@1.0.0',
  })
})

test('getPatchInfo(_, name, version) if name exists but name@version does not exist', () => {
  expect(_getPatchInfo({
    foo: '00000000000000000000000000000000',
  }, 'foo', '1.0.0')).toStrictEqual({
    hash: expect.any(String),
    key: 'foo',
  })
})

test('getPatchInfo(_, name, version) prioritizes name@version over name if both exist', () => {
  expect(_getPatchInfo({
    foo: '00000000000000000000000000000000',
    'foo@1.0.0': '00000000000000000000000000000000',
  }, 'foo', '1.0.0')).toStrictEqual({
    hash: expect.any(String),
    key: 'foo@1.0.0',
  })
})

test('getPatchInfo(_, name, version) does not access wrong name', () => {
  expect(_getPatchInfo({
    'bar@1.0.0': '00000000000000000000000000000000',
  }, 'foo', '1.0.0')).toBeUndefined()
})

test('getPatchInfo(_, name, version) does not access wrong version', () => {
  expect(_getPatchInfo({
    'foo@2.0.0': '00000000000000000000000000000000',
  }, 'foo', '1.0.0')).toBeUndefined()
})
