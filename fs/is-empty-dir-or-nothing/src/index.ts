import fs from 'node:fs'

export function isEmptyDirOrNothing (path: string) {
  try {
    const pathStat = fs.statSync(path)

    if (pathStat.isFile()) {
      return pathStat.size === 0
    }

    return isDirEmpty(path)
  } catch (error) {
    if (error && typeof error === 'object' && 'code' in error && error.code === 'ENOENT') {
      return true
    }

    // If an error other than ENOENT is thrown, we cannot promise that the path is empty
    return false
  }
}

function isDirEmpty (path: string) {
  const files = fs.readdirSync(path)
  return files.length === 0
}
