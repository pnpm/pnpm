import fs = require('mz/fs')
import path = require('path')

const dirs = new Set()

// write a stream to destination file
export default async function (
  fileDest: string,
  buffer: Buffer,
) {
  const dir = path.dirname(fileDest)
  if (!dirs.has(dir)) {
    await fs.mkdir(dir, { recursive: true })
    dirs.add(dir)
  }
  const fd = await fs.open(fileDest, 'w')
  await fs.write(fd, buffer, 0, buffer.length, 0)
  await fs.close(fd)
}
