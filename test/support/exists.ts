import fs = require('fs')

export default function exists (path: string) {
  try {
    return fs.statSync(path)
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
  }
  return null
}

export function existsSymlink (path: string) {
  try {
    return fs.lstatSync(path)
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
  }
  return null
}
