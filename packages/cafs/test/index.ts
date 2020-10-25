import createCafs, {
  checkFilesIntegrity,
  getFilePathInCafs,
} from '../src'
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
    const pkgFile = filesIndex['package.json']
    expect(pkgFile.size).toBe(1121)
    expect(pkgFile.mode).toBe(420)
    const { checkedAt, integrity } = await pkgFile.writeResult
    expect(typeof checkedAt).toBe('number')
    expect(integrity.toString()).toBe('sha512-8xCvrlC7W3TlwXxetv5CZTi53szYhmT7tmpXF/ttNthtTR9TC7Y7WJFPmJToHaSQ4uObuZyOARdOJYNYuTSbXA==')
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

describe('checkFilesIntegrity()', () => {
  it("doesn't fail if file was removed from the store", async () => {
    const storeDir = tempy.directory()
    expect(await checkFilesIntegrity(storeDir, {
      foo: {
        integrity: 'sha512-8xCvrlC7W3TlwXxetv5CZTi53szYhmT7tmpXF/ttNthtTR9TC7Y7WJFPmJToHaSQ4uObuZyOARdOJYNYuTSbXA==',
        mode: 420,
        size: 10,
      },
    })).toBeFalsy()
  })
})

test('file names are normalized when unpacking a tarball', async () => {
  const dest = tempy.directory()
  console.log(dest)
  const cafs = createCafs(dest)
  const filesIndex = await cafs.addFilesFromTarball(
    fs.createReadStream(path.join(__dirname, 'fixtures/colorize-semver-diff.tgz'))
  )
  expect(Object.keys(filesIndex).sort()).toStrictEqual([
    'LICENSE',
    'README.md',
    'lib/index.d.ts',
    'lib/index.js',
    'package.json',
  ])
})
