import fs from 'fs'
import {
  readMsgpackFile,
  readMsgpackFileSync,
  writeMsgpackFile,
  writeMsgpackFileSync,
} from '@pnpm/fs.msgpack-file'
import { temporaryDirectory } from 'tempy'

describe('json-file', () => {
  let tmpDir: string

  beforeEach(() => {
    tmpDir = temporaryDirectory()
  })

  test('writeFileSync and readFileSync', () => {
    const filePath = `${tmpDir}/test.json`
    const data = {
      foo: 'bar',
      baz: 123,
      nested: {
        a: 1,
        b: 2,
      },
    }

    writeMsgpackFileSync(filePath, data)
    expect(fs.existsSync(filePath)).toBe(true)

    const readData = readMsgpackFileSync(filePath)
    expect(readData).toEqual(data)
  })

  test('writeFile and readFile (async)', async () => {
    const filePath = `${tmpDir}/test-async.json`
    const data = {
      foo: 'bar',
      baz: 123,
      nested: {
        a: 1,
        b: 2,
      },
    }

    await writeMsgpackFile(filePath, data)
    expect(fs.existsSync(filePath)).toBe(true)

    const readData = await readMsgpackFile(filePath)
    expect(readData).toEqual(data)
  })

  test('it should serialize arrays of objects correctly', () => {
    const filePath = `${tmpDir}/records.json`
    const structure = { name: 'pkg', version: '1.0.0' }
    const data = [
      structure,
      structure,
      structure,
    ]

    writeMsgpackFileSync(filePath, data)
    const readData = readMsgpackFileSync<any>(filePath) // eslint-disable-line @typescript-eslint/no-explicit-any

    expect(readData).toHaveLength(3)
    expect(readData[0]).toEqual(structure)
    expect(readData[2]).toEqual(structure)
  })
})
