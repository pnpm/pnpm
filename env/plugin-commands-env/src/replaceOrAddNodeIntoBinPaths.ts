import path from 'path'

export function replaceOrAddNodeIntoBinPaths (binPaths: string[], baseDir: string, nodePath: string): string[] {
  baseDir = path.resolve(baseDir)
  if (!baseDir.endsWith(path.sep)) {
    baseDir += path.sep
  }

  const index = binPaths.findIndex(dir => dir.startsWith(baseDir))
  if (index < 0) {
    return [...binPaths, nodePath]
  } else {
    return [...binPaths.slice(0, index), nodePath, ...binPaths.slice(index + 1)]
  }
}
