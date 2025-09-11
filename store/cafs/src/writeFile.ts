import path from 'path'
import { writeFileWithRetry, mkdirSyncWithRetry } from '@pnpm/graceful-fs'

const dirs = new Set()

export function writeFile (
  fileDest: string,
  buffer: Buffer,
  mode?: number
): void {
  makeDirForFile(fileDest)
  writeFileWithRetry(fileDest, buffer, { mode })
}

function makeDirForFile (fileDest: string): void {
  const dir = path.dirname(fileDest)
  if (!dirs.has(dir)) {
    mkdirSyncWithRetry(dir, { recursive: true })
    dirs.add(dir)
  }
}
