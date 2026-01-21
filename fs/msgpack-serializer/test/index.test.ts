import fs from 'fs'
import { readFile, readFileSync, writeFile, writeFileSync } from '@pnpm/msgpack-serializer'
import { temporaryDirectory } from 'tempy'

describe('msgpack-serializer', () => {
  let tmpDir: string

  beforeEach(() => {
    tmpDir = temporaryDirectory()
  })

  test('writeFileSync and readFileSync', () => {
    const filePath = `${tmpDir}/test.mpk`
    const data = {
      foo: 'bar',
      baz: 123,
      nested: {
        a: 1,
        b: 2,
      },
    }

    writeFileSync(filePath, data)
    expect(fs.existsSync(filePath)).toBe(true)

    const readData = readFileSync(filePath)
    expect(readData).toEqual(data)
  })

  test('writeFile and readFile (async)', async () => {
    const filePath = `${tmpDir}/test-async.mpk`
    const data = {
      foo: 'bar',
      baz: 123,
      nested: {
        a: 1,
        b: 2,
      },
    }

    await writeFile(filePath, data)
    expect(fs.existsSync(filePath)).toBe(true)

    const readData = await readFile(filePath)
    expect(readData).toEqual(data)
  })

  test('it should support Map and Set serialization (moreTypes: true)', () => {
    const filePath = `${tmpDir}/map-set.mpk`
    const data = {
      map: new Map([['key1', 'value1'], ['key2', 'value2']]),
      set: new Set([1, 2, 3, 3]),
    }

    writeFileSync(filePath, data)
    const readData = readFileSync<any>(filePath) // eslint-disable-line @typescript-eslint/no-explicit-any

    expect(readData.map).toBeInstanceOf(Map)
    expect(readData.map.get('key1')).toBe('value1')
    expect(readData.set).toBeInstanceOf(Set)
    expect(readData.set.has(1)).toBe(true)
    expect(readData.set.size).toBe(3)
  })

  test('it should use record structures for optimization (useRecords: true)', () => {
    const filePath = `${tmpDir}/records.mpk`
    const structure = { name: 'pkg', version: '1.0.0' }
    const data = [
      structure,
      structure,
      structure,
    ]

    writeFileSync(filePath, data)
    const readData = readFileSync<any>(filePath) // eslint-disable-line @typescript-eslint/no-explicit-any

    expect(readData).toHaveLength(3)
    expect(readData[0]).toEqual(structure)
    expect(readData[2]).toEqual(structure)
  })
})


