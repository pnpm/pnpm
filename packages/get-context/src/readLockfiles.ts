import {
  LOCKFILE_VERSION,
  WANTED_LOCKFILE,
} from '@pnpm/constants'
import {
  createLockfileObject,
  existsWantedLockfile,
  Lockfile,
  readCurrentLockfile,
  readWantedLockfile,
  readWantedLockfileAndAutofixConflicts,
} from '@pnpm/lockfile-file'
import logger from '@pnpm/logger'
import isCI = require('is-ci')
import R = require('ramda')

export interface PnpmContext {
  currentLockfile: Lockfile
  existsCurrentLockfile: boolean
  existsWantedLockfile: boolean
  wantedLockfile: Lockfile
}

export default async function (
  opts: {
    autofixMergeConflicts: boolean
    force: boolean
    forceSharedLockfile: boolean
    projects: Array<{
      id: string
      rootDir: string
    }>
    lockfileDir: string
    registry: string
    useLockfile: boolean
    virtualStoreDir: string
  }
): Promise<{
    currentLockfile: Lockfile
    currentLockfileIsUpToDate: boolean
    existsCurrentLockfile: boolean
    existsWantedLockfile: boolean
    wantedLockfile: Lockfile
    lockfileHadConflicts: boolean
  }> {
  // ignore `pnpm-lock.yaml` on CI servers
  // a latest pnpm should not break all the builds
  const lockfileOpts = {
    ignoreIncompatible: opts.force || isCI,
    wantedVersion: LOCKFILE_VERSION,
  }
  const fileReads = [] as Array<Promise<Lockfile | undefined | null>>
  let lockfileHadConflicts: boolean = false
  if (opts.useLockfile) {
    if (opts.autofixMergeConflicts) {
      fileReads.push(
        readWantedLockfileAndAutofixConflicts(opts.lockfileDir, lockfileOpts)
          .then(({ lockfile, hadConflicts }) => {
            lockfileHadConflicts = hadConflicts
            return lockfile
          })
      )
    } else {
      fileReads.push(readWantedLockfile(opts.lockfileDir, lockfileOpts))
    }
  } else {
    if (await existsWantedLockfile(opts.lockfileDir)) {
      logger.warn({
        message: `A ${WANTED_LOCKFILE} file exists. The current configuration prohibits to read or write a lockfile`,
        prefix: opts.lockfileDir,
      })
    }
    fileReads.push(Promise.resolve(undefined))
  }
  fileReads.push(readCurrentLockfile(opts.virtualStoreDir, lockfileOpts))
  // eslint-disable-next-line @typescript-eslint/no-invalid-void-type
  const files = await Promise.all<Lockfile | null | undefined>(fileReads)
  const sopts = { lockfileVersion: LOCKFILE_VERSION }
  const importerIds = opts.projects.map((importer) => importer.id)
  const currentLockfile = files[1] ?? createLockfileObject(importerIds, sopts)
  for (const importerId of importerIds) {
    if (!currentLockfile.importers[importerId]) {
      currentLockfile.importers[importerId] = {
        specifiers: {},
      }
    }
  }
  const wantedLockfile = files[0] ??
    (currentLockfile && R.clone(currentLockfile)) ??
    createLockfileObject(importerIds, sopts)
  for (const importerId of importerIds) {
    if (!wantedLockfile.importers[importerId]) {
      wantedLockfile.importers[importerId] = {
        specifiers: {},
      }
    }
  }
  return {
    currentLockfile,
    currentLockfileIsUpToDate: R.equals(currentLockfile, wantedLockfile),
    existsCurrentLockfile: !!files[1],
    existsWantedLockfile: !!files[0],
    wantedLockfile,
    lockfileHadConflicts,
  }
}
