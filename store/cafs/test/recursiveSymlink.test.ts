import fs from 'node:fs'
import path from 'node:path'

import { temporaryDirectory } from 'tempy'

import { createCafs } from '../src/index.js'

test('addFilesFromDir does not loop infinitely on recursive symlinks', () => {
  const storeDir = temporaryDirectory()
  const srcDir = temporaryDirectory()

  fs.writeFileSync(path.join(srcDir, 'file.txt'), 'content')
  // Create a symlink pointing to the current directory
  fs.symlinkSync('.', path.join(srcDir, 'self'))

  const calves = createCafs(storeDir)
  const { filesIndex } = calves.addFilesFromDir(srcDir)

  expect(filesIndex.has('file.txt')).toBe(true)
  expect(filesIndex.has('self/file.txt')).toBe(false)
})
