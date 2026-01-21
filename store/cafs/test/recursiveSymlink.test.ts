import fs from 'fs'
import path from 'path'
import tempy from 'tempy'
import { createCafs } from '../src/index.js'

test('addFilesFromDir does not loop infinitely on recursive symlinks', () => {
  const storeDir = tempy.directory()
  const srcDir = tempy.directory()

  fs.writeFileSync(path.join(srcDir, 'file.txt'), 'content')
  // Create a symlink pointing to the current directory
  fs.symlinkSync('.', path.join(srcDir, 'self'))

  const cafs = createCafs(storeDir)
  const { filesIndex } = cafs.addFilesFromDir(srcDir)

  expect(filesIndex['file.txt']).toBeTruthy()
  expect(filesIndex['self/file.txt']).toBeFalsy()
})
