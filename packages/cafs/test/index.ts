import createCafs, { getFilePathInCafs } from '../src'
import fs = require('mz/fs')
import path = require('path')
import tempy = require('tempy')

describe('cafs', () => {
  it('unpack', async () => {
    const dest = tempy.directory()
    const cafs = createCafs(dest)
    const filesIndex = await cafs.addFilesFromTarball(
      fs.createReadStream(path.join(__dirname, '../__fixtures__/node-gyp-6.1.0.tgz'))
    )
    expect(Object.keys(filesIndex)).toHaveLength(121)
  })

  it('replaces an already existing file, if the integrity of it was broken', async () => {
    const storeDir = tempy.directory()
    const srcDir = path.join(__dirname, 'fixtures/one-file')
    const addFiles = () => createCafs(storeDir).addFilesFromDir(srcDir)

    let filesIndex = await addFiles()
    const { integrity } = await filesIndex['foo.txt'].writeResult

    // Modifying the file in the store
    const filePath = getFilePathInCafs(storeDir, integrity, 'nonexec')
    await fs.appendFile(filePath, 'bar')

    filesIndex = await addFiles()
    await filesIndex['foo.txt'].writeResult
    expect(await fs.readFile(filePath, 'utf8')).toBe('foo\n')
  })
})
