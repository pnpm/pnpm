import { expect, test } from '@jest/globals'

import { diffDir, DIR, extendFilesMap, type ExtendFilesMapStats, type InodeMap } from '../src/DirPatcher.js'

const fileStats = (ino: number): ExtendFilesMapStats => ({
  dev: 66,
  ino,
  isDirectory: () => false,
  isFile: () => true,
})

test('extendFilesMap() records paths named like Object.prototype members', async () => {
  const filesMap = new Map<string, string>([
    ['constructor/index.js', '/src/constructor/index.js'],
    ['toString', '/src/toString'],
    ['__proto__', '/src/__proto__'],
  ])
  const filesStats: Record<string, ExtendFilesMapStats> = {
    'constructor/index.js': fileStats(1),
    toString: fileStats(2),
    __proto__: fileStats(3),
  }

  const inodeMap = await extendFilesMap({ filesMap, filesStats })

  expect(Object.getPrototypeOf(inodeMap)).toBe(Object.prototype)
  expect(Object.keys(inodeMap).sort()).toStrictEqual(['.', '__proto__', 'constructor', 'constructor/index.js', 'toString'])
  expect(inodeMap.constructor).toBe(DIR)
  expect(inodeMap.toString).toBe('66:2')
  expect(Object.getOwnPropertyDescriptor(inodeMap, '__proto__')?.value).toBe('66:3')
})

test('diffDir() reports added and removed paths named like Object.prototype members', () => {
  const oldIndex: InodeMap = Object.fromEntries([['.', DIR], ['valueOf', '66:1']])
  const newIndex: InodeMap = Object.fromEntries([['.', DIR], ['constructor', DIR]])

  const diff = diffDir(oldIndex, newIndex)

  expect(diff.added).toStrictEqual([{ path: 'constructor', newValue: DIR }])
  expect(diff.removed).toStrictEqual([{ path: 'valueOf', oldValue: '66:1' }])
  expect(diff.modified).toStrictEqual([])
})
