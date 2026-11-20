import fs, { type Stats } from 'node:fs'
import path from 'node:path'

import { isError } from '@pnpm/error'
import gfs from '@pnpm/fs.graceful-fs'
import type {
  AddToStoreResult,
  FilesIndex,
  FileWriteResult,
} from '@pnpm/store.cafs-types'
import type { DependencyManifest } from '@pnpm/types'
import { isSubdir } from 'is-subdir'

import { parseJsonBufferSync } from './parseJson.js'
import { normalizeSymlinkTarget, SYMLINK_MODE } from './symlinks.js'

export function addFilesFromDir (
  addBuffer: (buffer: Buffer, mode: number) => FileWriteResult,
  dirname: string,
  opts: {
    files?: string[]
    includeNodeModules?: boolean
    readManifest?: boolean
    /**
     * Record symlinks whose targets {@link normalizeSymlinkTarget} accepts as
     * entries of type {@link SYMLINK_MODE}, instead of following them.
     */
    recordSymlinks?: boolean
  } = {}
): AddToStoreResult {
  // Resolve the package root to a canonical path for security validation
  const resolvedRoot = fs.realpathSync(dirname)
  const { files, hasUnrecordedSymlinks, symlinks } = opts.files
    ? statListedFiles(dirname, resolvedRoot, opts.files)
    : findFilesInDir(dirname, resolvedRoot, opts)
  const filesIndex = new Map() as FilesIndex
  const manifest = addFilesToIndex({ addBuffer, filesIndex, files, readManifest: opts.readManifest })
  addSymlinksToIndex(addBuffer, filesIndex, symlinks)
  return { manifest, filesIndex, hasUnrecordedSymlinks }
}

interface FoundFiles {
  files: File[]
  hasUnrecordedSymlinks: boolean
  symlinks: Symlink[]
}

function statListedFiles (dirname: string, resolvedRoot: string, listedFiles: string[]): FoundFiles {
  const files: File[] = []
  let hasUnrecordedSymlinks = false
  for (const file of listedFiles) {
    const absolutePath = path.join(dirname, file)
    const result = getStatIfContained(absolutePath, resolvedRoot)
    hasUnrecordedSymlinks ||= result.isSymbolicLink
    const { stat } = result
    if (!stat) {
      continue
    }
    files.push({
      absolutePath,
      relativePath: file,
      stat,
    })
  }
  return { files, hasUnrecordedSymlinks, symlinks: [] }
}

function addFilesToIndex (
  { addBuffer, filesIndex, files, readManifest }: {
    addBuffer: (buffer: Buffer, mode: number) => FileWriteResult
    filesIndex: FilesIndex
    files: File[]
    readManifest?: boolean
  }
): DependencyManifest | undefined {
  let manifest: DependencyManifest | undefined
  for (const { absolutePath, relativePath, stat } of files) {
    const buffer = gfs.readFileSync(absolutePath)
    if (readManifest && relativePath === 'package.json') {
      manifest = parseJsonBufferSync(buffer) as DependencyManifest
    }
    // Remove the file type information (regular file, directory, etc.) and leave just the permission bits (rwx for owner, group, and others)
    const mode = stat.mode & 0o777
    filesIndex.set(relativePath, {
      mode,
      size: stat.size,
      ...addBuffer(buffer, mode),
    })
  }
  return manifest
}

function addSymlinksToIndex (
  addBuffer: (buffer: Buffer, mode: number) => FileWriteResult,
  filesIndex: FilesIndex,
  symlinks: Symlink[]
): void {
  for (const { relativePath, target } of symlinks) {
    const buffer = Buffer.from(target, 'utf8')
    filesIndex.set(relativePath, {
      mode: SYMLINK_MODE,
      size: buffer.length,
      ...addBuffer(buffer, SYMLINK_MODE),
    })
  }
}

interface Symlink {
  relativePath: string
  target: string
}

interface File {
  relativePath: string
  absolutePath: string
  stat: Stats
}

/**
 * Resolves a path and validates it stays within the allowed root directory.
 * If the path is a symlink, resolves it and validates the target.
 * Returns a null stat if the path is missing, points outside the root, or has an inaccessible target.
 */
function getStatIfContained (
  absolutePath: string,
  rootDir: string
): { isSymbolicLink: boolean, stat: Stats | null } {
  let lstat: Stats
  try {
    lstat = fs.lstatSync(absolutePath)
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') {
      return { isSymbolicLink: false, stat: null }
    }
    throw err
  }
  if (lstat.isSymbolicLink()) {
    return {
      isSymbolicLink: true,
      stat: getSymlinkStatIfContained(absolutePath, rootDir)?.stat ?? null,
    }
  }
  let realPath: string
  try {
    realPath = fs.realpathSync(absolutePath)
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') {
      return { isSymbolicLink: false, stat: null }
    }
    throw err
  }
  if (!isSubdir(rootDir, realPath)) {
    return { isSymbolicLink: false, stat: null }
  }
  return { isSymbolicLink: false, stat: lstat }
}

/**
 * Validates a known symlink points within the allowed root directory.
 * Returns null if the symlink points outside the root or if target is inaccessible.
 */
function getSymlinkStatIfContained (
  absolutePath: string,
  rootDir: string
): { stat: Stats, realPath: string } | null {
  let realPath: string
  try {
    realPath = fs.realpathSync(absolutePath)
  } catch (err: unknown) {
    // Broken symlink or inaccessible target
    if (isError(err) && 'code' in err && err.code === 'ENOENT') {
      return null
    }
    throw err
  }
  // isSubdir returns true if realPath is within rootDir OR if they are equal
  if (!isSubdir(rootDir, realPath)) {
    return null // Symlink points outside package - skip
  }
  return { stat: fs.statSync(realPath), realPath }
}

function findFilesInDir (
  dir: string,
  rootDir: string,
  opts: { includeNodeModules?: boolean, recordSymlinks?: boolean }
): FoundFiles {
  const ctx: FindFilesContext = {
    filesList: [],
    includeNodeModules: opts.includeNodeModules ?? false,
    hasUnrecordedSymlinks: false,
    recordSymlinks: opts.recordSymlinks ?? false,
    rootDir,
    symlinks: [],
    visited: new Set([rootDir]),
  }
  findFiles(ctx, dir, '', rootDir)
  return { files: ctx.filesList, hasUnrecordedSymlinks: ctx.hasUnrecordedSymlinks, symlinks: ctx.symlinks }
}

interface FindFilesContext {
  filesList: File[]
  hasUnrecordedSymlinks: boolean
  includeNodeModules: boolean
  recordSymlinks: boolean
  symlinks: Symlink[]
  rootDir: string
  visited: Set<string>
}

function findFiles (
  ctx: FindFilesContext,
  dir: string,
  relativeDir: string,
  currentRealPath: string
): void {
  const dirents = fs.readdirSync(dir, { withFileTypes: true })
  for (const dirent of dirents) {
    const entry = toDirEntry(ctx, { dir, relativeDir, name: dirent.name })
    if (dirent.isSymbolicLink()) {
      const symlinkedDir = collectSymlink(ctx, entry)
      if (symlinkedDir) {
        descendIntoDir(ctx, entry, symlinkedDir)
      }
    } else if (dirent.isDirectory()) {
      descendIntoDir(ctx, entry, path.join(currentRealPath, dirent.name))
    } else {
      collectFile(ctx, entry)
    }
  }
}

interface DirEntry {
  absolutePath: string
  isExcludedNodeModules: boolean
  relativePath: string
}

function toDirEntry (
  ctx: FindFilesContext,
  { dir, relativeDir, name }: { dir: string, relativeDir: string, name: string }
): DirEntry {
  return {
    absolutePath: path.join(dir, name),
    isExcludedNodeModules: relativeDir === '' && name === 'node_modules' && !ctx.includeNodeModules,
    relativePath: `${relativeDir}${relativeDir ? '/' : ''}${name}`,
  }
}

/**
 * Records the symlink or the file it points to.
 * Returns the real path of the target when it is a directory that should be walked.
 */
function collectSymlink (ctx: FindFilesContext, entry: DirEntry): string | undefined {
  if (entry.isExcludedNodeModules) {
    return undefined
  }
  if (ctx.recordSymlinks && recordSymlink(ctx, entry)) {
    return undefined
  }
  ctx.hasUnrecordedSymlinks = true
  const res = getSymlinkStatIfContained(entry.absolutePath, ctx.rootDir)
  if (!res) {
    return undefined
  }
  if (res.stat.isDirectory()) {
    return res.realPath
  }
  ctx.filesList.push({
    relativePath: entry.relativePath,
    absolutePath: entry.absolutePath,
    stat: res.stat,
  })
  return undefined
}

function recordSymlink (ctx: FindFilesContext, entry: DirEntry): boolean {
  const target = normalizeSymlinkTarget(entry.relativePath, fs.readlinkSync(entry.absolutePath))
  if (target == null) {
    return false
  }
  ctx.symlinks.push({ relativePath: entry.relativePath, target })
  return true
}

function descendIntoDir (ctx: FindFilesContext, entry: DirEntry, realDir: string): void {
  if (ctx.visited.has(realDir) || entry.isExcludedNodeModules) {
    return
  }
  ctx.visited.add(realDir)
  findFiles(ctx, entry.absolutePath, entry.relativePath, realDir)
  ctx.visited.delete(realDir)
}

function collectFile (ctx: FindFilesContext, entry: DirEntry): void {
  let stat: Stats
  try {
    stat = fs.statSync(entry.absolutePath)
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') {
      return
    }
    throw err
  }
  ctx.filesList.push({
    relativePath: entry.relativePath,
    absolutePath: entry.absolutePath,
    stat,
  })
}
