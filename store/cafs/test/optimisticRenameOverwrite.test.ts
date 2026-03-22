import fs from 'node:fs'
import path from 'node:path'

import { temporaryDirectory } from 'tempy'

import { optimisticRenameOverwrite } from '../src/writeBufferToCafs.js'

test("optimisticRenameOverwrite() doesn't crash if target file exists", () => {
  const tempDir = temporaryDirectory()
  const dest = path.join(tempDir, 'file')
  fs.writeFileSync(dest, '', 'utf8')
  optimisticRenameOverwrite(`${dest}_tmp`, dest)
})
