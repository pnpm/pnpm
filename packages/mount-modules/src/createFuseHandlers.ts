import fs from 'fs'
import { getFilePathInCafs, getFilePathByModeInCafs, PackageFilesIndex } from '@pnpm/cafs'
import { Lockfile, readWantedLockfile, PackageSnapshot } from '@pnpm/lockfile-file'
import {
  nameVerFromPkgSnapshot,
} from '@pnpm/lockfile-utils'
import * as schemas from 'hyperdrive-schemas'
import loadJsonFile from 'load-json-file'
import Fuse from 'fuse-native'
import * as cafsExplorer from './cafsExplorer'
import makeVirtualNodeModules from './makeVirtualNodeModules'

const TIME = new Date()
const STAT_DEFAULT = {
  mtime: TIME,
  atime: TIME,
  ctime: TIME,
  nlink: 1,
  uid: process.getuid ? process.getuid() : 0,
  gid: process.getgid ? process.getgid() : 0,
}

export default async function createFuseHandlers (lockfileDir: string, cafsDir: string) {
  const lockfile = await readWantedLockfile(lockfileDir, { ignoreIncompatible: true })
  if (!lockfile) throw new Error('Cannot generate a .pnp.js without a lockfile')
  return createFuseHandlersFromLockfile(lockfile, lockfileDir, cafsDir)
}

/* eslint-disable standard/no-callback-literal */
export function createFuseHandlersFromLockfile (lockfile: Lockfile, lockfileDir: string, cafsDir: string) {
  const pkgSnapshotCache = new Map<string, { name: string, version: string, pkgSnapshot: PackageSnapshot, index: PackageFilesIndex }>()
  const virtualNodeModules = makeVirtualNodeModules(lockfile, lockfileDir)
  return {
    open (p: string, flags: string | number, cb: (exitCode: number, fd?: number) => void) {
      const dirEnt = getDirEnt(p)
      if (dirEnt?.entryType !== 'index') {
        cb(-1)
        return
      }
      const fileInfo = dirEnt.index.files[dirEnt.subPath]
      if (!fileInfo) {
        cb(-1)
        return
      }
      const filePathInStore = getFilePathByModeInCafs(cafsDir, fileInfo.integrity, fileInfo.mode)
      fs.open(filePathInStore, flags, (err, fd) => {
        if (err) {
          cb(-1)
          return
        }
        cb(0, fd)
      })
    },
    release (p: string, fd: number, cb: (exitCode: number) => void) {
      fs.close(fd, (err) => cb(err ? -1 : 0))
    },
    read (p: string, fd: number, buffer: Buffer, length: number, position: number, cb: (readBytes: number) => void) {
      fs.read(fd, buffer, position, length, position, (err, bytesRead) => {
        if (err) {
          cb(-1)
          return
        }
        cb(bytesRead)
      })
    },
    readlink (p: string, cb: (returnCode: number, target?: string) => void) {
      const dirEnt = getDirEnt(p)
      if (dirEnt?.entryType !== 'symlink') {
        cb(Fuse.ENOENT)
        return
      }
      cb(0, dirEnt.target)
    },
    /* eslint-disable-next-line @typescript-eslint/no-explicit-any */
    getattr (p: string, cb: (returnCode: number, files?: any) => void) {
      const dirEnt = getDirEnt(p)
      if (!dirEnt) {
        cb(Fuse.ENOENT)
        return
      }
      if (dirEnt.entryType === 'directory' || dirEnt.entryType === 'index' && !dirEnt.subPath) {
        cb(0, schemas.Stat.directory({
          ...STAT_DEFAULT,
          size: 1,
        }))
        return
      }
      if (dirEnt.entryType === 'symlink') {
        cb(0, schemas.Stat.symlink({
          ...STAT_DEFAULT,
          size: 1,
        }))
        return
      }
      if (dirEnt.entryType === 'index') {
        switch (cafsExplorer.dirEntityType(dirEnt.index, dirEnt.subPath)) {
        case 'file':
          cb(0, schemas.Stat.file({
            ...STAT_DEFAULT,
            size: dirEnt.index.files[dirEnt.subPath].size,
          }))
          return
        case 'directory':
          cb(0, schemas.Stat.directory({
            ...STAT_DEFAULT,
            size: 1,
          }))
          return
        default:
          cb(Fuse.ENOENT)
          return
        }
      }
      cb(Fuse.ENOENT)
    },
    readdir,
  }
  function readdir (p: string, cb: (returnCode: number, files?: string[]) => void) {
    const dirEnt = getDirEnt(p)
    if (dirEnt?.entryType === 'index') {
      const dirEnts = cafsExplorer.readdir(dirEnt.index, dirEnt.subPath)
      if (dirEnts.length === 0) {
        cb(Fuse.ENOENT)
        return
      }
      cb(0, dirEnts)
      return
    }
    if (!dirEnt || dirEnt.entryType !== 'directory') {
      cb(Fuse.ENOENT)
      return
    }
    cb(0, Object.keys(dirEnt.entries))
  }
  function getDirEnt (p: string) {
    let currentDirEntry = virtualNodeModules
    const parts = p === '/' ? [] : p.split('/')
    parts.shift()
    while (parts.length && currentDirEntry && currentDirEntry.entryType === 'directory') {
      currentDirEntry = currentDirEntry.entries[parts.shift()!]
    }
    if (currentDirEntry?.entryType === 'index') {
      const pkg = getPkgInfo(currentDirEntry.depPath, cafsDir)
      if (!pkg) {
        return null
      }
      return {
        ...currentDirEntry,
        index: pkg.index,
        subPath: parts.join('/'),
      }
    }
    return currentDirEntry
  }
  function getPkgInfo (depPath: string, cafsDir: string) {
    if (!pkgSnapshotCache.has(depPath)) {
      const pkgSnapshot = lockfile.packages?.[depPath]
      if (!pkgSnapshot) return undefined
      const indexPath = getFilePathInCafs(cafsDir, pkgSnapshot.resolution['integrity'], 'index')
      pkgSnapshotCache.set(depPath, {
        ...nameVerFromPkgSnapshot(depPath, pkgSnapshot),
        pkgSnapshot,
        index: loadJsonFile.sync<PackageFilesIndex>(indexPath), // TODO: maybe make it async?
      })
    }
    return pkgSnapshotCache.get(depPath)
  }
}
