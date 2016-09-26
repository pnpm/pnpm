import fs = require('mz/fs')

export default async function exists (path: string) {
  try {
    return await fs.stat(path)
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
  }
  return null
}

export async function existsSymlink (path: string) {
  try {
    return await fs.lstat(path)
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
  }
  return null
}
