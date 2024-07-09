import fs from 'node:fs'
import path from 'node:path'
import os from 'node:os'
import { isEmptyDirOrNothing } from '@pnpm/fs.is-empty-dir-or-nothing'

describe('isEmptyDirOrNothing', () => {
  it('should return true on a non-existent path', () => {
    const nonExistentPath = path.resolve(
      __dirname,
      './__fixtures__/not-exists'
    )
    const result = isEmptyDirOrNothing(nonExistentPath)
    expect(result).toBe(true)
  })

  it('should return true on an empty directory', () => {
    const emptyDirPath = fs.mkdtempSync(`${os.tmpdir()}/empty-dir`)
    const result = isEmptyDirOrNothing(emptyDirPath)
    expect(result).toBe(true)
  })

  it('should return false on a directory with a file in it', () => {
    const dirWithFilesPath = path.resolve(
      __dirname,
      './__fixtures__/dir-with-files'
    )
    const result = isEmptyDirOrNothing(dirWithFilesPath)
    expect(result).toBe(false)
  })

  it('should return false on a directory with a directory', () => {
    const dirWithFilesPath = path.resolve(
      __dirname,
      './__fixtures__/dir-with-dirs'
    )
    const result = isEmptyDirOrNothing(dirWithFilesPath)
    expect(result).toBe(false)
  })
})
