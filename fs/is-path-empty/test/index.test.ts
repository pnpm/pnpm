import fs from 'node:fs/promises'
import path from 'node:path'
import os from 'node:os'
import { isPathEmpty } from '@pnpm/is-path-empty'

describe('isPathEmpty', () => {
  it('should return true on a non-existent path', async () => {
    const nonExistentPath = path.resolve(__dirname, './__fixtures__/not-exists')
    const result = await isPathEmpty(nonExistentPath)
    expect(result).toBe(true)
  })

  it('should return true on an empty directory', async () => {
    const emptyDirPath = await fs.mkdtemp(`${os.tmpdir()}/empty-dir`)
    const result = await isPathEmpty(emptyDirPath)
    expect(result).toBe(true)
  })

  it('should return false on a directory with a file in it', async () => {
    const dirWithFilesPath = path.resolve(__dirname, './__fixtures__/dir-with-files')
    const result = await isPathEmpty(dirWithFilesPath)
    expect(result).toBe(false)
  })

  it('should return false on a directory with a directory', async () => {
    const dirWithFilesPath = path.resolve(__dirname, './__fixtures__/dir-with-dirs')
    const result = await isPathEmpty(dirWithFilesPath)
    expect(result).toBe(false)
  })
})