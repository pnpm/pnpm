import fs from 'fs'
import path from 'path'
import { temporaryDirectory } from 'tempy'
import { createCafs } from '../src/index.js'

test('addFilesFromDir does not loop infinitely on recursive symlinks', () => {
  const storeDir = temporaryDirectory()
  const srcDir = temporaryDirectory()

  fs.writeFileSync(path.join(srcDir, 'file.txt'), 'content')
  // Create a symlink pointing to the current directory
  fs.symlinkSync('.', path.join(srcDir, 'self'))

  const cafs = createCafs(storeDir)
  const { filesIndex } = cafs.addFilesFromDir(srcDir)

  expect(filesIndex.has('file.txt')).toBe(true)
  expect(filesIndex.has('self/file.txt')).toBe(false)
})
