import fs, { type BigIntStats } from 'node:fs'
import path from 'node:path'

export function hardlinkedDirMatches (dir: string, files: Map<string, string>, keepModulesDir: boolean): boolean {
  if (files.size === 0) return false
  const directories = expectedDirectories(files)
  if (!directories) return false
  try {
    const rootStat = fs.lstatSync(dir)
    if (!rootStat.isDirectory() || rootStat.isSymbolicLink()) return false
    return treeMatches(dir, { files, directories, keepModulesDir })
  } catch {
    return false
  }
}

function expectedDirectories (files: Map<string, string>): Set<string> | undefined {
  const directories = new Set<string>()
  for (const relative of files.keys()) {
    if (!isReusableFilePath(relative)) return undefined
    for (let parent = path.posix.dirname(relative); parent !== '.'; parent = path.posix.dirname(parent)) directories.add(parent)
  }
  return directories
}

function isReusableFilePath (relative: string): boolean {
  if (relative === 'node_modules' || relative.startsWith('node_modules/')) return false
  if (path.isAbsolute(relative) || relative.includes('\\')) return false
  return relative.split('/').every(part => part !== '' && part !== '.' && part !== '..')
}

interface ExpectedTree {
  files: Map<string, string>
  directories: Set<string>
  keepModulesDir: boolean
}

function treeMatches (root: string, expected: ExpectedTree): boolean {
  const pending = ['']
  let matched = 0
  for (let relative = pending.pop(); relative !== undefined; relative = pending.pop()) {
    const count = matchingFilesInDirectory(root, relative, { expected, pending })
    if (count === undefined) return false
    matched += count
  }
  return matched === expected.files.size
}

function matchingFilesInDirectory (root: string, relative: string, { expected, pending }: { expected: ExpectedTree, pending: string[] }): number | undefined {
  let matched = 0
  for (const entry of fs.readdirSync(path.join(root, relative), { withFileTypes: true })) {
    const entryPath = relative ? `${relative}/${entry.name}` : entry.name
    const kind = matchingEntryKind(path.join(root, entryPath), entryPath, expected)
    if (!kind) return undefined
    if (kind === 'directory') pending.push(entryPath)
    if (kind === 'file') matched++
  }
  return matched
}

function matchingEntryKind (targetPath: string, relative: string, expected: ExpectedTree): 'file' | 'directory' | 'preserved' | undefined {
  const target = fs.lstatSync(targetPath, { bigint: true })
  if (target.isSymbolicLink()) return undefined
  if (target.isDirectory()) {
    if (expected.keepModulesDir && relative === 'node_modules') return 'preserved'
    return expected.directories.has(relative) ? 'directory' : undefined
  }
  const source = expected.files.get(relative)
  return source && filesHaveSameIdentity(target, fs.lstatSync(source, { bigint: true })) ? 'file' : undefined
}

export function filesHaveSameIdentity (target: BigIntStats, source: BigIntStats): boolean {
  return target.isFile() && source.isFile() && target.ino !== 0n && target.dev !== 0n &&
    target.ino === source.ino && target.dev === source.dev
}
