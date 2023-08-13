import fs from 'fs'
import path from 'path'

const dirs = new Set()

export function writeFile (
  fileDest: string,
  buffer: Buffer,
  mode?: number
) {
  makeDirForFile(fileDest)
  fs.writeFileSync(fileDest, buffer, { mode })
}

function makeDirForFile (fileDest: string) {
  const dir = path.dirname(fileDest)
  if (!dirs.has(dir)) {
    fs.mkdirSync(dir, { recursive: true })
    dirs.add(dir)
  }
}
