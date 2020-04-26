import fs = require('mz/fs')
import path = require('path')

const dirs = new Set()

// write a stream to destination file
export default async function (
  fileDest: string,
  buffer: Buffer,
  mode?: number,
) {
  const dir = path.dirname(fileDest)
  if (!dirs.has(dir)) {
    await fs.mkdir(dir, { recursive: true })
    dirs.add(dir)
  }
  await fs.writeFile(fileDest, buffer, { mode })
}
