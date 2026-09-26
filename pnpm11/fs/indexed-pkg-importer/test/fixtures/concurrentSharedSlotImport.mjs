import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { importIndexedDir } from '../../lib/importIndexedDir.js'

const [role, newDir, srcDir, readyFile, releaseFile] = process.argv.slice(2)
if ([role, newDir, srcDir, readyFile, releaseFile].some((arg) => arg == null)) {
  throw new Error('expected role, target, source, ready, and release arguments')
}

let paused = false
const importFile = (src, dest) => {
  if (role === 'partial-writer' && !paused && path.basename(dest) === 'index.js') {
    paused = true
    fs.writeFileSync(dest, 'module.exports =')
    fs.writeFileSync(path.join(newDir, 'writer-owned.txt'), 'the first importer is still using this directory')
    fs.writeFileSync(readyFile, '')
    waitForFile(releaseFile)
    return
  }
  linkAdoptingExisting(src, dest)
}

importIndexedDir(
  { importFile, importFileAtomic: role === 'partial-writer' ? importFile : linkAdoptingExisting },
  newDir,
  new Map([
    ['index.js', path.join(srcDir, 'index.js')],
    ['package.json', path.join(srcDir, 'package.json')],
  ]),
  { safeToSkip: true }
)

function linkAdoptingExisting (src, dest) {
  try {
    fs.linkSync(src, dest)
  } catch (err) {
    if (!(util.types.isNativeError(err) && 'code' in err && err.code === 'EEXIST')) throw err
  }
}

function waitForFile (file) {
  const sleepBuffer = new Int32Array(new SharedArrayBuffer(4))
  const deadline = Date.now() + 20_000
  while (!fs.existsSync(file)) {
    if (Date.now() >= deadline) throw new Error(`timed out waiting for ${file}`)
    Atomics.wait(sleepBuffer, 0, 0, 10)
  }
}
