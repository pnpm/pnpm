import path from 'path'
import fs from '@pnpm/graceful-fs'

const dirs = new Set()

export function writeFile (
  fileDest: string,
  buffer: Buffer,
  mode?: number
): void {
  makeDirForFile(fileDest)
  fs.writeFileSync(fileDest, buffer, { mode })
}

function makeDirForFile (fileDest: string): void {
  const dir = path.dirname(fileDest)
  if (!dirs.has(dir)) {
    fs.mkdirSync(dir, { recursive: true })
    dirs.add(dir)
  }
}
