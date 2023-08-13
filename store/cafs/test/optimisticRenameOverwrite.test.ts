import fs from 'fs'
import path from 'path'
import tempy from 'tempy'
import { optimisticRenameOverwrite } from '../src/writeBufferToCafs'

test("optimisticRenameOverwrite() doesn't crash if target file exists", () => {
  const tempDir = tempy.directory()
  const dest = path.join(tempDir, 'file')
  fs.writeFileSync(dest, '', 'utf8')
  optimisticRenameOverwrite(`${dest}_tmp`, dest)
})
