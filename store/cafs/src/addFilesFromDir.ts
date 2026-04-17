import type { Stats } from 'node:fs'
import fsp from 'node:fs/promises'
import path from 'node:path'
import util from 'node:util'

import gfs from '@pnpm/fs.graceful-fs'
import type {
  AddToStoreResult,
  FilesIndex,
  FileWriteResult,
} from '@pnpm/store.cafs-types'
import type { DependencyManifest } from '@pnpm/types'
import { isSubdir } from 'is-subdir'

import type { JsonParseCache } from './jsonCache.js'
import { parseJsonBufferSync } from './parseJson.js'

export async function addFilesFromDir (
  addBuffer: (buffer: Buffer, mode: number) => FileWriteResult,
  jsonCache: JsonParseCache | undefined,
  dirname: string,
  opts: {
    files?: string[]
    includeNodeModules?: boolean
    readManifest?: boolean
  } = {}
): Promise<AddToStoreResult> {
  const filesIndex = new Map() as FilesIndex
  let manifest: DependencyManifest | undefined
  let files: File[]
  const resolvedRoot = await fsp.realpath(dirname)
  if (opts.files) {
    files = []
    for (const file of opts.files) {
      const absolutePath = path.join(dirname, file)
      // eslint-disable-next-line no-await-in-loop
      const stat = await getStatIfContained(absolutePath, resolvedRoot)
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
    files = await findFilesInDir(dirname, resolvedRoot, opts)
  }
  for (const { absolutePath, relativePath, stat } of files) {
    // eslint-disable-next-line no-await-in-loop
    const buffer = await gfs.readFile(absolutePath)
    const mode = stat.mode & 0o777
    const addBufferResult = addBuffer(buffer, mode)
    if (opts.readManifest && relativePath === 'package.json') {
      manifest = parseJsonBufferSync(buffer, jsonCache, addBufferResult.digest) as DependencyManifest
    }
    filesIndex.set(relativePath, {
      mode,
      size: stat.size,
      ...addBufferResult,
    })
  }
  return { manifest, filesIndex }
}

interface File {
  relativePath: string
  absolutePath: string
  stat: Stats
}

async function getStatIfContained (
  absolutePath: string,
  rootDir: string
): Promise<Stats | null> {
  let lstat: Stats
  try {
    lstat = await fsp.lstat(absolutePath)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return null
    }
    throw err
  }
  if (lstat.isSymbolicLink()) {
    return (await getSymlinkStatIfContained(absolutePath, rootDir))?.stat ?? null
  }
  return lstat
}

async function getSymlinkStatIfContained (
  absolutePath: string,
  rootDir: string
): Promise<{ stat: Stats, realPath: string } | null> {
  let realPath: string
  try {
    realPath = await fsp.realpath(absolutePath)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return null
    }
    throw err
  }
  if (!isSubdir(rootDir, realPath)) {
    return null
  }
  return { stat: await fsp.stat(realPath), realPath }
}

async function findFilesInDir (dir: string, rootDir: string, opts: { includeNodeModules?: boolean }): Promise<File[]> {
  const files: File[] = []
  const ctx: FindFilesContext = {
    filesList: files,
    includeNodeModules: opts.includeNodeModules ?? false,
    rootDir,
    visited: new Set([rootDir]),
  }
  await findFiles(ctx, dir, '', rootDir)
  return files
}

interface FindFilesContext {
  filesList: File[]
  includeNodeModules: boolean
  rootDir: string
  visited: Set<string>
}

async function findFiles (
  ctx: FindFilesContext,
  dir: string,
  relativeDir: string,
  currentRealPath: string
): Promise<void> {
  const files = await fsp.readdir(dir, { withFileTypes: true })
  for (const file of files) {
    const relativeSubdir = `${relativeDir}${relativeDir ? '/' : ''}${file.name}`
    const absolutePath = path.join(dir, file.name)
    let nextRealDir: string | undefined

    if (file.isSymbolicLink()) {
      // eslint-disable-next-line no-await-in-loop
      const res = await getSymlinkStatIfContained(absolutePath, ctx.rootDir)
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
        // eslint-disable-next-line no-await-in-loop
        await findFiles(ctx, absolutePath, relativeSubdir, nextRealDir)
        ctx.visited.delete(nextRealDir)
      }
      continue
    }

    let stat: Stats
    try {
      // eslint-disable-next-line no-await-in-loop
      stat = await fsp.stat(absolutePath)
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
