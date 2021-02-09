import { promises as fs } from 'fs'
import path = require('path')

const dirs = new Set()

export default async function (
  fileDest: string,
  buffer: Buffer,
  mode?: number
) {
  await makeDirForFile(fileDest)
  await fs.writeFile(fileDest, buffer, { mode })
}

async function makeDirForFile (fileDest: string) {
  const dir = path.dirname(fileDest)
  if (!dirs.has(dir)) {
    await fs.mkdir(dir, { recursive: true })
    dirs.add(dir)
  }
}
