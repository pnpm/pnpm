// cspell:ignore ents
import fs from 'node:fs'

import Fuse from 'fuse-native'
import loadJsonFile from 'load-json-file'
import * as schemas from 'hyperdrive-schemas'

import { readWantedLockfile } from '@pnpm/lockfile-file'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile-utils'
import { getFilePathByModeInCafs, getFilePathInCafs } from '@pnpm/store.cafs'
import type { Lockfile, PackageSnapshot, PackageFilesIndex, TarballResolution } from '@pnpm/types'

import * as cafsExplorer from './cafsExplorer.js'
import { makeVirtualNodeModules } from './makeVirtualNodeModules.js'

const TIME = new Date()

const STAT_DEFAULT = {
  mtime: TIME,
  atime: TIME,
  ctime: TIME,
  nlink: 1,
  uid: process.getuid ? process.getuid() : 0,
  gid: process.getgid ? process.getgid() : 0,
}

export async function createFuseHandlers(lockfileDir: string, cafsDir: string): Promise<{
  open(p: string, flags: string | number, cb: (exitCode: number, fd?: number | undefined) => void): void;
  release(p: string, fd: number, cb: (exitCode: number) => void): void;
  read(p: string, fd: number, buffer: Buffer, length: number, position: number, cb: (readBytes: number) => void): void;
  readlink(p: string, cb: (returnCode: number, target?: string | undefined) => void): void;
  getattr(p: string, cb: (returnCode: number, files?: string[] | undefined) => void): void;
  readdir: (p: string, cb: (returnCode: number, files?: string[] | undefined) => void) => void;
}> {
  const lockfile = await readWantedLockfile(lockfileDir, {
    ignoreIncompatible: true,
  })

  if (lockfile == null) {
    throw new Error('Cannot generate a .pnp.cjs without a lockfile')
  }

  return createFuseHandlersFromLockfile(lockfile, cafsDir)
}

export function createFuseHandlersFromLockfile(
  lockfile: Lockfile,
  cafsDir: string
): {
    open(p: string, flags: string | number, cb: (exitCode: number, fd?: number | undefined) => void): void;
    release(p: string, fd: number, cb: (exitCode: number) => void): void;
    read(p: string, fd: number, buffer: Buffer, length: number, position: number, cb: (readBytes: number) => void): void;
    readlink(p: string, cb: (returnCode: number, target?: string | undefined) => void): void;
    getattr(p: string, cb: (returnCode: number, files?: string[] | undefined) => void): void;
    readdir: (p: string, cb: (returnCode: number, files?: string[] | undefined) => void) => void;
  } {
  const pkgSnapshotCache = new Map<
    string,
    {
      name: string
      version: string
      pkgSnapshot: PackageSnapshot
      index: PackageFilesIndex
    }
  >()

  const virtualNodeModules = makeVirtualNodeModules(lockfile)

  return {
    open(
      p: string,
      flags: string | number,
      cb: (exitCode: number, fd?: number) => void
    ) {
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

      const filePathInStore = getFilePathByModeInCafs(
        cafsDir,
        fileInfo.integrity,
        fileInfo.mode
      )

      fs.open(filePathInStore, flags, (err, fd) => {
        if (err != null) {
          cb(-1)
          return
        }
        // eslint-disable-next-line n/no-callback-literal
        cb(0, fd)
      })
    },
    release(p: string, fd: number, cb: (exitCode: number) => void) {
      fs.close(fd, (err) => {
        cb(err != null ? -1 : 0)
      })
    },
    read(
      p: string,
      fd: number,
      buffer: Buffer,
      length: number,
      position: number,
      cb: (readBytes: number) => void
    ) {
      fs.read(fd, buffer, 0, length, position, (err, bytesRead) => {
        if (err != null) {
          cb(-1)
          return
        }
        cb(bytesRead)
      })
    },
    readlink(p: string, cb: (returnCode: number, target?: string | undefined) => void) {
      const dirEnt = getDirEnt(p)

      if (dirEnt?.entryType !== 'symlink') {
        cb(Fuse.ENOENT)
        return
      }

      // eslint-disable-next-line n/no-callback-literal
      cb(0, dirEnt.target)
    },

    getattr(p: string, cb: (returnCode: number, files?: string[] | undefined) => void) {
      const dirEnt = getDirEnt(p)

      if (dirEnt == null) {
        cb(Fuse.ENOENT)

        return
      }

      if (
        dirEnt.entryType === 'directory' ||
        (dirEnt.entryType === 'index' && !dirEnt.subPath)
      ) {
        // eslint-disable-next-line n/no-callback-literal
        cb(
          0,
          schemas.Stat.directory({
            ...STAT_DEFAULT,
            size: 1,
          })
        )

        return
      }

      if (dirEnt.entryType === 'symlink') {
        // eslint-disable-next-line n/no-callback-literal
        cb(
          0,
          schemas.Stat.symlink({
            ...STAT_DEFAULT,
            size: 1,
          })
        )

        return
      }

      if (dirEnt.entryType === 'index') {
        switch (cafsExplorer.dirEntityType(dirEnt.index, dirEnt.subPath)) {
          case 'file': {
            const file = dirEnt.index.files[dirEnt.subPath]

            if (file) {
            // eslint-disable-next-line n/no-callback-literal
              cb(
                0,
                schemas.Stat.file({
                  ...STAT_DEFAULT,
                  mode: file.mode,
                  size: file.size,
                })
              )
            }

            return
          }

          case 'directory': {
            // eslint-disable-next-line n/no-callback-literal
            cb(
              0,
              schemas.Stat.directory({
                ...STAT_DEFAULT,
                size: 1,
              })
            )

            return
          }

          default: {
            cb(Fuse.ENOENT)

            return
          }
        }
      }

      cb(Fuse.ENOENT)
    },
    readdir,
  }

  function readdir(
    p: string,
    cb: (returnCode: number, files?: string[] | undefined) => void
  ) {
    const dirEnt = getDirEnt(p)
    if (dirEnt?.entryType === 'index') {
      const dirEnts = cafsExplorer.readdir(dirEnt.index, dirEnt.subPath)
      if (dirEnts.length === 0) {
        cb(Fuse.ENOENT)
        return
      }
      // eslint-disable-next-line n/no-callback-literal
      cb(0, dirEnts)
      return
    }
    if (dirEnt == null || dirEnt.entryType !== 'directory') {
      cb(Fuse.ENOENT)
      return
    }
    // eslint-disable-next-line n/no-callback-literal
    cb(0, Object.keys(dirEnt.entries))
  }
  function getDirEnt(p: string) {
    let currentDirEntry = virtualNodeModules

    const parts = p === '/' ? [] : p.split('/')

    parts.shift()

    while (
      parts.length > 0 &&
      currentDirEntry &&
      currentDirEntry.entryType === 'directory'
    ) {
      const entry = currentDirEntry.entries[parts.shift() ?? '']

      if (entry) {
        currentDirEntry = entry
      }
    }

    if (currentDirEntry?.entryType === 'index') {
      const pkg = getPkgInfo(currentDirEntry.depPath, cafsDir)

      if (pkg == null) {
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

  function getPkgInfo(depPath: string, cafsDir: string): {
    name: string;
    version: string;
    pkgSnapshot: PackageSnapshot;
    index: PackageFilesIndex;
  } | undefined {
    if (!pkgSnapshotCache.has(depPath)) {
      const pkgSnapshot = lockfile.packages?.[depPath]

      if (pkgSnapshot == null) return undefined

      const indexPath = getFilePathInCafs(
        cafsDir,
        (pkgSnapshot.resolution as TarballResolution).integrity ?? '',
        'index'
      )

      pkgSnapshotCache.set(depPath, {
        ...nameVerFromPkgSnapshot(depPath, pkgSnapshot),
        pkgSnapshot,
        index: loadJsonFile.sync<PackageFilesIndex>(indexPath), // TODO: maybe make it async?
      })
    }

    return pkgSnapshotCache.get(depPath)
  }
}
