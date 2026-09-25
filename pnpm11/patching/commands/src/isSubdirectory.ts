import path from 'node:path'

interface PathUtils {
  isAbsolute: (path: string) => boolean
  relative: (from: string, to: string) => string
  sep: string
}

export function isSubdirectory (parentDir: string, childPath: string, pathUtils: PathUtils = path): boolean {
  const relativePath = pathUtils.relative(parentDir, childPath)

  return relativePath === '' || (
    relativePath !== '..' &&
    !relativePath.startsWith(`..${pathUtils.sep}`) &&
    !pathUtils.isAbsolute(relativePath)
  )
}
