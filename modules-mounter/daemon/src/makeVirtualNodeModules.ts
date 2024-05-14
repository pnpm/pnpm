import path from 'path'
import { type Lockfile } from '@pnpm/lockfile-file'
import { DEPENDENCIES_FIELDS } from '@pnpm/types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile-utils'
import * as dp from '@pnpm/dependency-path'
import normalize from 'normalize-path'

interface DirDirEntry {
  entryType: 'directory'
  entries: Record<string, DirEntry>
}

type DirEntry = {
  entryType: 'index'
  depPath: string
} | {
  entryType: 'symlink'
  target: string
} | DirDirEntry

export function makeVirtualNodeModules (lockfile: Lockfile): DirEntry {
  const entries: Record<string, DirEntry> = {
    '.pnpm': {
      entryType: 'directory',
      entries: createVirtualStoreDir(lockfile),
    },
  }
  for (const depType of DEPENDENCIES_FIELDS) {
    for (const [depName, ref] of Object.entries(lockfile.importers['.'][depType] ?? {})) {
      const symlink: DirEntry = {
        entryType: 'symlink',
        target: `./.pnpm/${dp.depPathToFilename(dp.refToRelative(ref, depName)!, 120)}/node_modules/${depName}`,
      }
      addDirEntry(entries, depName, symlink)
    }
  }
  return {
    entryType: 'directory',
    entries,
  }
}

function createVirtualStoreDir (lockfile: Lockfile): Record<string, DirEntry> {
  const rootDir = {} as Record<string, DirEntry>
  for (const [depPath, pkgSnapshot] of Object.entries(lockfile.packages ?? {})) {
    const { name } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
    const pkgNodeModules = {} as Record<string, DirEntry>
    const currentPath = dp.depPathToFilename(depPath, 120)
    const pkgDir: DirEntry = {
      entryType: 'index',
      depPath,
    }
    addDirEntry(pkgNodeModules, name, pkgDir)
    for (const [depName, ref] of Object.entries({ ...pkgSnapshot.dependencies, ...pkgSnapshot.optionalDependencies })) {
      const symlink: DirEntry = {
        entryType: 'symlink',
        target: normalize(path.relative(`${currentPath}/node_modules/`, `${dp.depPathToFilename(dp.refToRelative(ref, depName)!, 120)}/node_modules/${depName}`)),
      }
      addDirEntry(pkgNodeModules, depName, symlink)
    }
    addDirEntry(rootDir, currentPath, {
      entryType: 'directory',
      entries: {
        node_modules: {
          entryType: 'directory',
          entries: pkgNodeModules,
        },
      },
    })
  }
  return rootDir
}

function addDirEntry (target: Record<string, DirEntry>, subPath: string[] | string, newEntry: DirEntry): void {
  const subPathArray = typeof subPath === 'string' ? subPath.split('/') : subPath
  const p = subPathArray.shift()!
  if (subPathArray.length > 0) {
    if (!target[p]) {
      target[p] = {
        entryType: 'directory',
        entries: {},
      } as DirEntry
    } else if (target[p].entryType !== 'directory') {
      throw new Error()
    }
    addDirEntry((target[p] as DirDirEntry).entries, subPathArray, newEntry)
  } else {
    target[p] = newEntry
  }
}
