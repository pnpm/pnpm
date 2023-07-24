import fs from 'fs'
import path from 'path'
import tempy from 'tempy'
import { optimisticRenameOverwrite } from '../src/writeBufferToCafs'

test("optimisticRenameOverwrite() doesn't crash if target file exists", async () => {
  const tempDir = tempy.directory()
  const dest = path.join(tempDir, 'file')
  fs.writeFileSync(dest, '', 'utf8')
  await optimisticRenameOverwrite(`${dest}_tmp`, dest)
})
