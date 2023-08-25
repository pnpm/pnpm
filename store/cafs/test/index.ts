import fs from 'fs'
import { type DependencyManifest } from '@pnpm/types'
import pDefer from 'p-defer'
import path from 'path'
import tempy from 'tempy'
import {
  createCafs,
  checkPkgFilesIntegrity,
  getFilePathInCafs,
} from '../src'

describe('cafs', () => {
  it('unpack', async () => {
    const dest = tempy.directory()
    const cafs = createCafs(dest)
    const filesIndex = cafs.addFilesFromTarball(
      fs.readFileSync(path.join(__dirname, '../__fixtures__/node-gyp-6.1.0.tgz'))
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
    const manifest = pDefer<DependencyManifest>()
    const addFiles = async () => createCafs(storeDir).addFilesFromDir(srcDir, manifest)

    let filesIndex = await addFiles()
    const { integrity } = await filesIndex['foo.txt'].writeResult

    // Modifying the file in the store
    const filePath = getFilePathInCafs(storeDir, integrity, 'nonexec')
    fs.appendFileSync(filePath, 'bar')

    filesIndex = await addFiles()
    await filesIndex['foo.txt'].writeResult
    expect(fs.readFileSync(filePath, 'utf8')).toBe('foo\n')
    expect(await manifest.promise).toEqual(undefined)
  })

  it('ignores broken symlinks when traversing subdirectories', async () => {
    const storeDir = tempy.directory()
    const srcDir = path.join(__dirname, 'fixtures/broken-symlink')
    const manifest = pDefer<DependencyManifest>()
    const addFiles = async () => createCafs(storeDir).addFilesFromDir(srcDir, manifest)

    const filesIndex = await addFiles()
    expect(filesIndex[path.join('subdir', 'should-exist.txt')]).toBeDefined()
  })
})

describe('checkPkgFilesIntegrity()', () => {
  it("doesn't fail if file was removed from the store", async () => {
    const storeDir = tempy.directory()
    expect(await checkPkgFilesIntegrity(storeDir, {
      files: {
        foo: {
          integrity: 'sha512-8xCvrlC7W3TlwXxetv5CZTi53szYhmT7tmpXF/ttNthtTR9TC7Y7WJFPmJToHaSQ4uObuZyOARdOJYNYuTSbXA==',
          mode: 420,
          size: 10,
        },
      },
    })).toBeFalsy()
  })
})

test('file names are normalized when unpacking a tarball', async () => {
  const dest = tempy.directory()
  const cafs = createCafs(dest)
  const filesIndex = cafs.addFilesFromTarball(
    fs.readFileSync(path.join(__dirname, 'fixtures/colorize-semver-diff.tgz'))
  )
  expect(Object.keys(filesIndex).sort()).toStrictEqual([
    'LICENSE',
    'README.md',
    'lib/index.d.ts',
    'lib/index.js',
    'package.json',
  ])
})

test('broken magic in tarball headers is handled gracefully', async () => {
  const dest = tempy.directory()
  const cafs = createCafs(dest)
  cafs.addFilesFromTarball(
    fs.readFileSync(path.join(__dirname, 'fixtures/jquery.dirtyforms-2.0.0.tgz'))
  )
})
