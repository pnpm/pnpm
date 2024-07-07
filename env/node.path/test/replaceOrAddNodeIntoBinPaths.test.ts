import path from 'path'
import { replaceOrAddNodeIntoBinPaths } from '../src/replaceOrAddNodeIntoBinPaths'

const baseDir = path.resolve('nodejs')

test('adds a path to an empty array', () => {
  const binPaths: string[] = []
  replaceOrAddNodeIntoBinPaths(binPaths, baseDir, path.join(baseDir, '20.0.0', 'bin'))
  expect(binPaths).toStrictEqual([
    path.join(baseDir, '20.0.0', 'bin'),
  ])
})

test('adds a path to a non-empty array without nodejs bin dirs', () => {
  const binPaths = [
    path.resolve('node_modules/.bin'),
  ]
  replaceOrAddNodeIntoBinPaths(binPaths, baseDir, path.join(baseDir, '20.0.0', 'bin'))
  expect(binPaths).toStrictEqual([
    path.resolve('node_modules/.bin'),
    path.join(baseDir, '20.0.0', 'bin'),
  ])
})

test('replaces a nodejs bin dirs', () => {
  const binPaths = [
    path.resolve('foo'),
    path.resolve('bar'),
    path.join(baseDir, '18.0.0', 'bin'),
    path.resolve('baz'),
  ]
  replaceOrAddNodeIntoBinPaths(binPaths, baseDir, path.join(baseDir, '20.0.0', 'bin'))
  expect(binPaths).toStrictEqual([
    path.resolve('foo'),
    path.resolve('bar'),
    path.join(baseDir, '20.0.0', 'bin'),
    path.resolve('baz'),
  ])
})
