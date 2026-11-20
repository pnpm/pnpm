import { existsSync, lstatSync, mkdirSync, realpathSync } from 'node:fs'
import { dirname, isAbsolute, relative, resolve, sep } from 'node:path'

export function resolveBenchOutputPath (output) {
  const benchDir = resolve('.bench')
  const outputPath = resolve(output)
  const relativePath = relative(benchDir, outputPath)
  if (relativePath === '' || isOutside(relativePath)) {
    throw new Error(`Output path must be under .bench/: ${output}`)
  }
  ensureBenchDir(benchDir, output)
  assertNotSymlink(outputPath, output)
  assertNoSymlinkAncestors(benchDir, dirname(outputPath), output)

  const canonicalBenchDir = realpathSync(benchDir)
  const canonicalParent = realpathSync(nearestExistingAncestor(dirname(outputPath)))
  if (!isSameOrChild(canonicalBenchDir, canonicalParent)) {
    throw new Error(`Output path must be under .bench/: ${output}`)
  }
  return outputPath
}

function ensureBenchDir (benchDir, output) {
  if (!existsSync(benchDir)) {
    mkdirSync(benchDir)
    return
  }
  const stats = lstatSync(benchDir)
  if (stats.isSymbolicLink() || !stats.isDirectory()) {
    throw new Error(`Output path must be under .bench/: ${output}`)
  }
}

function assertNoSymlinkAncestors (benchDir, outputParent, output) {
  const relativeParent = relative(benchDir, outputParent)
  if (relativeParent === '') return

  let current = benchDir
  for (const segment of relativeParent.split(sep)) {
    current = resolve(current, segment)
    assertNotSymlink(current, output)
  }
}

function assertNotSymlink (path, output) {
  if (existsSync(path) && lstatSync(path).isSymbolicLink()) {
    throw new Error(`Output path must be under .bench/: ${output}`)
  }
}

function nearestExistingAncestor (path) {
  let current = path
  while (!existsSync(current)) {
    const parent = dirname(current)
    if (parent === current) return current
    current = parent
  }
  return current
}

function isSameOrChild (base, target) {
  return !isOutside(relative(base, target))
}

function isOutside (relativePath) {
  return relativePath === '..' || relativePath.startsWith(`..${sep}`) || isAbsolute(relativePath)
}
