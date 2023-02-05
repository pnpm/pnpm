import path from 'path'
import { splitLockfile } from '@pnpm/split-lockfile'
import { loadLockfile } from './utils'

test('splitLockfile() should works for single importer', async () => {
  const lockfile = await loadLockfile(path.resolve(__dirname, './fixture/single-import'))
  const result = splitLockfile(lockfile)
  expect(Object.keys(result).length).toBe(1)
  expect(result['.']).toStrictEqual(lockfile)
})

test('splitLockfile() should works for v5.4', async () => {
  const lockfile = await loadLockfile(path.resolve(__dirname, './fixture/v5.4'))
  const result = splitLockfile(lockfile)
  expect(Object.keys(result).length).toBe(4)
  expect(result['.']).toMatchSnapshot('packages/root')
  expect(result['packages/a']).toMatchSnapshot('packages/a')
  expect(result['packages/b']).toMatchSnapshot('packages/b')
  expect(result['packages/c']).toMatchSnapshot('packages/c')
})

test('splitLockfile() should works for v6', async () => {
  const lockfile = await loadLockfile(path.resolve(__dirname, './fixture/v6'))
  const result = splitLockfile(lockfile)
  expect(Object.keys(result).length).toBe(4)
  expect(result['.']).toMatchSnapshot('packages/root')
  expect(result['packages/a']).toMatchSnapshot('packages/a')
  expect(result['packages/b']).toMatchSnapshot('packages/b')
  expect(result['packages/c']).toMatchSnapshot('packages/c')
})

test('splitLockfile() should add info in root lockfile', async () => {
  const lockfile = await loadLockfile(path.resolve(__dirname, './fixture/info'))
  const result = splitLockfile(lockfile)
  expect(Object.keys(result).length).toBe(3)
  expect(result['.']).toMatchSnapshot('packages/root')
})
