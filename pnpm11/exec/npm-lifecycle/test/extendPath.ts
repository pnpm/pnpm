import path from 'node:path'

import { expect, test } from '@jest/globals'

import { extendPath } from '../src/extendPath.js'

const separator = process.platform === 'win32' ? ';' : ':'

test('the path to node-gyp should be added after the path to node_modules/.bin', () => {
  const p = extendPath(process.cwd(), '', { nodeGypBinDir: 'node_gyp', extraBinPaths: [] })
  expect(p.indexOf('.bin')).toBeLessThan(p.indexOf('node_gyp'))
})

test('every node_modules above a nested package contributes its .bin, nearest first', () => {
  const root = path.resolve('project')
  const wd = path.join(root, 'node_modules', 'dep', 'node_modules', 'nested')
  const p = extendPath(wd, 'original', { nodeGypBinDir: 'node_gyp', extraBinPaths: ['extra'] })
  expect(p.split(separator)).toStrictEqual([
    path.join(wd, 'node_modules', '.bin'),
    path.join(root, 'node_modules', 'dep', 'node_modules', '.bin'),
    path.join(root, 'node_modules', '.bin'),
    'node_gyp',
    'extra',
    'original',
  ])
})

const testOnWindows = process.platform === 'win32' ? test : test.skip

testOnWindows('a drive-qualified path is split at its node_modules directories', () => {
  const p = extendPath('C:\\project\\node_modules\\dep', undefined, { nodeGypBinDir: 'node_gyp' })
  expect(p.split(separator)).toStrictEqual([
    'C:\\project\\node_modules\\dep\\node_modules\\.bin',
    'C:\\project\\node_modules\\.bin',
    'node_gyp',
  ])
})

test('wdBinDir replaces the working directory\'s own .bin, leaving the packages above it on node_modules', () => {
  const root = path.resolve('project')
  const wd = path.join(root, 'node_modules', 'dep')
  const p = extendPath(wd, 'original', {
    nodeGypBinDir: 'node_gyp',
    wdBinDir: path.join(wd, 'vendor', '.bin'),
    extraBinPaths: ['extra'],
  })
  expect(p.split(separator)).toStrictEqual([
    path.join(wd, 'vendor', '.bin'),
    path.join(root, 'node_modules', '.bin'),
    'node_gyp',
    'extra',
    'original',
  ])
})
