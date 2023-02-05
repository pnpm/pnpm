import type { Lockfile } from '@pnpm/lockfile-types'
import type { LockfileWalkerStep } from '@pnpm/lockfile-walker'
import { lockfileWalkerGroupImporterSteps } from '@pnpm/lockfile-walker'
import { pickLockfileInfo } from './utils'

export function splitLockfile (lockfile: Lockfile): Record<string, Lockfile> {
  const importerIds = Object.keys(lockfile.importers)
  if (importerIds.length === 1) {
    return {
      '.': lockfile,
    }
  }
  const walkers = lockfileWalkerGroupImporterSteps(lockfile, importerIds)
  const entires: Array<[string, Lockfile]> = walkers.map(({ importerId, step }) => {
    const newLockfile = walk(importerId, lockfile, step)
    if (importerId === '.') {
      return [importerId, {
        ...newLockfile,
        ...pickLockfileInfo(lockfile),
      }]
    } else {
      return [importerId, newLockfile]
    }
  })

  return Object.fromEntries(entires)
}

function walk (importerId: string, lockfile: Lockfile, firstStep: LockfileWalkerStep): Lockfile {
  const importers = lockfile.importers[importerId]
  const newLockfile: Lockfile = {
    importers: {
      '.': importers,
    },
    packages: {},
    lockfileVersion: lockfile.lockfileVersion,
  }

  dfs(newLockfile, firstStep)

  if (newLockfile.packages && Object.keys(newLockfile.packages).length === 0) {
    delete newLockfile.packages
  }

  return newLockfile

  function dfs (newLockfile: Lockfile, step: LockfileWalkerStep) {
    step.dependencies.forEach(dep => {
      if (!newLockfile.packages) {
        throw Error('unreachable')
      }
      if (!newLockfile.packages[dep.depPath]) {
        newLockfile.packages[dep.depPath] = dep.pkgSnapshot
        dfs(newLockfile, dep.next())
      }
    })
  }
}
