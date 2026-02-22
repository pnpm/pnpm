import util from 'util'
import fs, { type Stats } from 'fs'
import path from 'path'
import {
  type AddToStoreResult,
  type FilesIndex,
  type FileWriteResult,
} from '@pnpm/cafs-types'
import gfs from '@pnpm/graceful-fs'
import { type DependencyManifest } from '@pnpm/types'
import isSubdir from 'is-subdir'
import { parseJsonBufferSync } from './parseJson.js'

export function addFilesFromDir (
  addBuffer: (buffer: Buffer, mode: number) => FileWriteResult,
  dirname: string,
  opts: {
    files?: string[]
    includeNodeModules?: boolean
    readManifest?: boolean
  } = {}
): AddToStoreResult {
  const filesIndex = new Map() as FilesIndex
  let manifest: DependencyManifest | undefined
  let files: File[]
  // Resolve the package root to a canonical path for security validation
  const resolvedRoot = fs.realpathSync(dirname)
  if (opts.files) {
    files = []
    for (const file of opts.files) {
      const absolutePath = path.join(dirname, file)
      const stat = getStatIfContained(absolutePath, resolvedRoot)
      if (!stat) {
        continue
      }
      files.push({
        absolutePath,
        relativePath: file,
        stat,
      })
    }
  } else {
    files = findFilesInDir(dirname, resolvedRoot, opts)
  }
  for (const { absolutePath, relativePath, stat } of files) {
    const buffer = gfs.readFileSync(absolutePath)
    if (opts.readManifest && relativePath === 'package.json') {
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
  return { manifest, filesIndex }
}

interface File {
  relativePath: string
  absolutePath: string
  stat: Stats
}

/**
 * Resolves a path and validates it stays within the allowed root directory.
 * If the path is a symlink, resolves it and validates the target.
 * Returns null if the path is a symlink pointing outside the root, or if target is inaccessible.
 */
function getStatIfContained (
  absolutePath: string,
  rootDir: string
): Stats | null {
  let lstat: Stats
  try {
    lstat = fs.lstatSync(absolutePath)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return null
    }
    throw err
  }
  if (lstat.isSymbolicLink()) {
    return getSymlinkStatIfContained(absolutePath, rootDir)?.stat ?? null
  }
  return lstat
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
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
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

function findFilesInDir (dir: string, rootDir: string, opts: { includeNodeModules?: boolean }): File[] {
  const files: File[] = []
  const ctx: FindFilesContext = {
    filesList: files,
    includeNodeModules: opts.includeNodeModules ?? false,
    rootDir,
    visited: new Set([rootDir]),
  }
  findFiles(ctx, dir, '', rootDir)
  return files
}

interface FindFilesContext {
  filesList: File[]
  includeNodeModules: boolean
  rootDir: string
  visited: Set<string>
}

function findFiles (
  ctx: FindFilesContext,
  dir: string,
  relativeDir: string,
  currentRealPath: string
): void {
  const files = fs.readdirSync(dir, { withFileTypes: true })
  for (const file of files) {
    const relativeSubdir = `${relativeDir}${relativeDir ? '/' : ''}${file.name}`
    const absolutePath = path.join(dir, file.name)
    let nextRealDir: string | undefined

    if (file.isSymbolicLink()) {
      const res = getSymlinkStatIfContained(absolutePath, ctx.rootDir)
      if (!res) {
        continue
      }
      if (res.stat.isDirectory()) {
        nextRealDir = res.realPath
      } else {
        ctx.filesList.push({
          relativePath: relativeSubdir,
          absolutePath,
          stat: res.stat,
        })
        continue
      }
    } else if (file.isDirectory()) {
      nextRealDir = path.join(currentRealPath, file.name)
    }

    if (nextRealDir) {
      if (ctx.visited.has(nextRealDir)) continue
      if (relativeDir !== '' || file.name !== 'node_modules' || ctx.includeNodeModules) {
        ctx.visited.add(nextRealDir)
        findFiles(ctx, absolutePath, relativeSubdir, nextRealDir)
        ctx.visited.delete(nextRealDir)
      }
      continue
    }

    let stat: Stats
    try {
      stat = fs.statSync(absolutePath)
    } catch (err: unknown) {
      if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
        continue
      }
      throw err
    }
    ctx.filesList.push({
      relativePath: relativeSubdir,
      absolutePath,
      stat,
    })
  }
}
