// cspell:ignore ents
import fs from 'node:fs'

import { type LockfileObject, type PackageSnapshot, readWantedLockfile, type TarballResolution } from '@pnpm/lockfile.fs'
import {
  nameVerFromPkgSnapshot,
} from '@pnpm/lockfile.utils'
import { getFilePathByModeInCafs, type PackageFilesIndex } from '@pnpm/store.cafs'
import { pickStoreIndexKey, StoreIndex } from '@pnpm/store.index'
import type { DepPath } from '@pnpm/types'
import Fuse from 'fuse-native'
import schemas from 'hyperdrive-schemas'

import * as cafsExplorer from './cafsExplorer.js'
import { type DirEntry, makeVirtualNodeModules } from './makeVirtualNodeModules.js'

const TIME = new Date()
const STAT_DEFAULT = {
  mtime: TIME,
  atime: TIME,
  ctime: TIME,
  nlink: 1,
  uid: process.getuid ? process.getuid() : 0,
  gid: process.getgid ? process.getgid() : 0,
}

export interface FuseHandlers {
  open: (entryPath: string, flags: string | number, cb: (exitCode: number, fd?: number) => void) => void
  release: (entryPath: string, fd: number, cb: (exitCode: number) => void) => void
  read: (entryPath: string, fd: number, buffer: Buffer, length: number, position: number, cb: (readBytes: number) => void) => void
  readlink: (entryPath: string, cb: (returnCode: number, target?: string) => void) => void
  /* eslint-disable-next-line @typescript-eslint/no-explicit-any -- the stat objects come from hyperdrive-schemas, which has no types */
  getattr: (entryPath: string, cb: (returnCode: number, files?: any) => void) => void
  readdir: (entryPath: string, cb: (returnCode: number, files?: string[]) => void) => void
}

export async function createFuseHandlers (lockfileDir: string, storeDir: string): Promise<FuseHandlers> {
  const lockfile = await readWantedLockfile(lockfileDir, { ignoreIncompatible: true })
  if (lockfile == null) throw new Error('Cannot generate a .pnp.cjs without a lockfile')
  return createFuseHandlersFromLockfile(lockfile, storeDir)
}

export function createFuseHandlersFromLockfile (lockfile: LockfileObject, storeDir: string): FuseHandlers {
  const storeIndex = new StoreIndex(storeDir)
  const pkgSnapshotCache = new Map<string, { name: string, version: string, pkgSnapshot: PackageSnapshot, index: PackageFilesIndex }>()
  const virtualNodeModules = makeVirtualNodeModules(lockfile)
  const resolveDirEnt = (entryPath: string) =>
    getDirEnt(virtualNodeModules, entryPath, (depPath) => getPkgInfo(depPath, lockfile, storeIndex, pkgSnapshotCache))

  return {
    getattr: (entryPath, cb) => handleGetattr(resolveDirEnt(entryPath), cb),
    open: (entryPath, flags, cb) => handleOpen(resolveDirEnt(entryPath), storeDir, flags, cb),
    read: handleRead,
    readdir: (entryPath, cb) => handleReaddir(resolveDirEnt(entryPath), cb),
    readlink: (entryPath, cb) => handleReadlink(resolveDirEnt(entryPath), cb),
    release: handleRelease,
  }
}

function handleOpen (
  dirEnt: ResolvedDirEnt | null | undefined,
  storeDir: string,
  flags: string | number,
  cb: (exitCode: number, fd?: number) => void
): void {
  if (dirEnt?.entryType !== 'index') {
    cb(-1)
    return
  }
  const fileInfo = dirEnt.index.files.get(dirEnt.subPath)
  if (!fileInfo) {
    cb(-1)
    return
  }
  const filePathInStore = getFilePathByModeInCafs(storeDir, fileInfo.digest, fileInfo.mode)
  fs.open(filePathInStore, flags, (err, fd) => {
    if (err != null) {
      cb(-1)
      return
    }
    cb(0, fd)
  })
}

function handleRelease (entryPath: string, fd: number, cb: (exitCode: number) => void): void {
  fs.close(fd, (err) => {
    cb((err != null) ? -1 : 0)
  })
}

function handleRead (
  entryPath: string,
  fd: number,
  buffer: Buffer,
  length: number,
  position: number,
  cb: (readBytes: number) => void
): void {
  fs.read(fd, buffer, 0, length, position, (err, bytesRead) => {
    if (err != null) {
      cb(-1)
      return
    }
    cb(bytesRead)
  })
}

function handleReadlink (
  dirEnt: ResolvedDirEnt | null | undefined,
  cb: (returnCode: number, target?: string) => void
): void {
  if (dirEnt?.entryType !== 'symlink') {
    cb(Fuse.ENOENT)
    return
  }
  cb(0, dirEnt.target)
}

/* eslint-disable-next-line @typescript-eslint/no-explicit-any -- the stat objects come from hyperdrive-schemas, which has no types */
function handleGetattr (dirEnt: ResolvedDirEnt | null | undefined, cb: (returnCode: number, files?: any) => void): void {
  if (dirEnt == null) {
    cb(Fuse.ENOENT)
    return
  }
  if (dirEnt.entryType === 'directory' || (dirEnt.entryType === 'index' && !dirEnt.subPath)) {
    cb(0, schemas.Stat.directory({ ...STAT_DEFAULT, size: 1 }))
    return
  }
  if (dirEnt.entryType === 'symlink') {
    cb(0, schemas.Stat.symlink({ ...STAT_DEFAULT, size: 1 }))
    return
  }
  handleIndexGetattr(dirEnt, cb)
}

/* eslint-disable-next-line @typescript-eslint/no-explicit-any -- the stat objects come from hyperdrive-schemas, which has no types */
function handleIndexGetattr (dirEnt: ResolvedIndexDirEnt, cb: (returnCode: number, files?: any) => void): void {
  switch (cafsExplorer.dirEntityType(dirEnt.index, dirEnt.subPath)) {
    case 'file': {
      const fileInfo = dirEnt.index.files.get(dirEnt.subPath)!
      cb(0, schemas.Stat.file({
        ...STAT_DEFAULT,
        mode: fileInfo.mode,
        size: fileInfo.size,
      }))
      return
    }
    case 'directory':
      cb(0, schemas.Stat.directory({ ...STAT_DEFAULT, size: 1 }))
      return
    default:
      cb(Fuse.ENOENT)
  }
}

function handleReaddir (
  dirEnt: ResolvedDirEnt | null | undefined,
  cb: (returnCode: number, files?: string[]) => void
): void {
  if (dirEnt?.entryType === 'index') {
    const dirEnts = cafsExplorer.readdir(dirEnt.index, dirEnt.subPath)
    if (dirEnts.length === 0) {
      cb(Fuse.ENOENT)
      return
    }
    cb(0, dirEnts)
    return
  }
  if (dirEnt == null || dirEnt.entryType !== 'directory') {
    cb(Fuse.ENOENT)
    return
  }
  cb(0, Object.keys(dirEnt.entries))
}

interface PkgInfo {
  index: PackageFilesIndex
  name: string
  pkgSnapshot: PackageSnapshot
  version: string
}

type ResolvedIndexDirEnt = {
  entryType: 'index'
  depPath: string
  index: PackageFilesIndex
  subPath: string
}

type ResolvedDirEnt =
  | ResolvedIndexDirEnt
  | { entryType: 'symlink', target: string }
  | { entryType: 'directory', entries: Record<string, DirEntry> }

function getDirEnt (
  virtualNodeModules: DirEntry,
  entryPath: string,
  getPkg: (depPath: string) => PkgInfo | undefined
): ResolvedDirEnt | null {
  let currentDirEntry: DirEntry | undefined = virtualNodeModules
  const parts = entryPath === '/' ? [] : entryPath.split('/')
  parts.shift()
  while (parts.length > 0 && currentDirEntry && currentDirEntry.entryType === 'directory') {
    currentDirEntry = currentDirEntry.entries[parts.shift()!]
  }
  if (currentDirEntry?.entryType === 'index') {
    const pkg = getPkg(currentDirEntry.depPath)
    if (pkg == null) {
      return null
    }
    return {
      ...currentDirEntry,
      index: pkg.index,
      subPath: parts.join('/'),
    }
  }
  return currentDirEntry ?? null
}

function getPkgInfo (
  depPath: string,
  lockfile: LockfileObject,
  storeIndex: StoreIndex,
  pkgSnapshotCache: Map<string, { name: string, version: string, pkgSnapshot: PackageSnapshot, index: PackageFilesIndex }>
) {
  if (!pkgSnapshotCache.has(depPath)) {
    const pkgSnapshot = lockfile.packages?.[depPath as DepPath]
    if (pkgSnapshot == null) return undefined
    const nameVer = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
    const resolution = pkgSnapshot.resolution as TarballResolution
    const pkgId = nameVer.nonSemverVersion ?? `${nameVer.name}@${nameVer.version}`
    const pkgIndexFilePath = pickStoreIndexKey(resolution, pkgId, { built: true })
    const pkgIndex = storeIndex.get(pkgIndexFilePath) as PackageFilesIndex
    pkgSnapshotCache.set(depPath, {
      ...nameVer,
      pkgSnapshot,
      index: pkgIndex,
    })
  }
  return pkgSnapshotCache.get(depPath)
}
