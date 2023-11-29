import fs from 'node:fs/promises'

const isDirEmpty = async (path: string) => {
  const files = await fs.readdir(path)
  return files.length === 0
}

export const isPathEmpty = async (path: string) => {
  try {
    const pathStat = await fs.stat(path)

    if (pathStat.isFile()) {
      return pathStat.size === 0
    }

    // path is dir
    return await isDirEmpty(path)
  } catch (error) {
    if (error && typeof error === 'object' && 'code' in error && error.code === 'ENOENT') {
      return true
    }

    // If an error other than ENOENT is thrown, we cannot promise that the path is empty
    return false
  }
}
