import { promises as fs } from 'fs'
import path from 'path'
import gfs from '@pnpm/graceful-fs'

const dirs = new Set()

export async function writeFile (
  fileDest: string,
  buffer: Buffer,
  mode?: number
) {
  await makeDirForFile(fileDest)
  await gfs.writeFile(fileDest, buffer, { mode })
}

async function makeDirForFile (fileDest: string) {
  const dir = path.dirname(fileDest)
  if (!dirs.has(dir)) {
    await fs.mkdir(dir, { recursive: true })
    dirs.add(dir)
  }
}
