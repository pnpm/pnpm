import path from 'path'

export function replaceOrAddNodeIntoBinPaths (binPaths: string[], baseDir: string, nodePath: string): void {
  baseDir = path.resolve(baseDir)
  if (!baseDir.endsWith(path.sep)) {
    baseDir += path.sep
  }

  const index = binPaths.findIndex(dir => dir.startsWith(baseDir))
  if (index < 0) {
    binPaths.push(nodePath)
  } else {
    binPaths[index] = nodePath
  }
}
