import fs from 'fs'
import path from 'path'
import ssri from 'ssri'
import tempy from 'tempy'
import { pathTemp, writeBufferToCafs } from '../src/writeBufferToCafs'

describe('writeBufferToCafs', () => {
  it('should not fail if a file already exists at the temp file location', () => {
    const cafsDir = tempy.directory()
    const fileDest = 'abc'
    const buffer = Buffer.from('abc')
    const fullFileDest = path.join(cafsDir, fileDest)
    fs.writeFileSync(pathTemp(fullFileDest), 'ccc', 'utf8')
    writeBufferToCafs(new Map(), cafsDir, buffer, fileDest, 420, ssri.fromData(buffer))
    expect(fs.readFileSync(fullFileDest, 'utf8')).toBe('abc')
  })
})
