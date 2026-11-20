import fs from 'node:fs'
import path from 'node:path'

import { importIndexedDir } from '../../lib/importIndexedDir.js'

const [newDir, srcDir, releaseFile] = process.argv.slice(2)
if ([newDir, srcDir, releaseFile].some((arg) => arg == null)) {
  throw new Error('expected target, source, and release arguments')
}

waitForFile(releaseFile)
importIndexedDir(
  { importFile: fs.copyFileSync, importFileAtomic: fs.copyFileSync },
  newDir,
  new Map([
    ['index.js', path.join(srcDir, 'index.js')],
    ['package.json', path.join(srcDir, 'package.json')],
  ]),
  { keepModulesDir: true }
)

function waitForFile (file) {
  const sleepBuffer = new Int32Array(new SharedArrayBuffer(4))
  const deadline = Date.now() + 20_000
  while (!fs.existsSync(file)) {
    if (Date.now() >= deadline) throw new Error(`timed out waiting for ${file}`)
    Atomics.wait(sleepBuffer, 0, 0, 10)
  }
}
