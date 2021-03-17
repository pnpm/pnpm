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
import isCI from 'is-ci'
import * as R from 'ramda'

export interface PnpmContext {
  currentLockfile: Lockfile
  existsCurrentLockfile: boolean
  existsWantedLockfile: boolean
  wantedLockfile: Lockfile
}

export default async function (
  opts: {
    force: boolean
    forceSharedLockfile: boolean
    frozenLockfile: boolean
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
    if (!opts.frozenLockfile) {
      fileReads.push(
        (async () => {
          try {
            const { lockfile, hadConflicts } = await readWantedLockfileAndAutofixConflicts(opts.lockfileDir, lockfileOpts)
            lockfileHadConflicts = hadConflicts
            return lockfile
          } catch (err) {
            logger.warn({
              message: `Ignoring broken lockfile at ${opts.lockfileDir}: ${err.message as string}`,
              prefix: opts.lockfileDir,
            })
            return undefined
          }
        })()
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
  fileReads.push(
    (async () => {
      try {
        return await readCurrentLockfile(opts.virtualStoreDir, lockfileOpts)
      } catch (err) {
        logger.warn({
          message: `Ignoring broken lockfile at ${opts.virtualStoreDir}: ${err.message as string}`,
          prefix: opts.lockfileDir,
        })
        return undefined
      }
    })()
  )
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
    existsCurrentLockfile: files[1] != null,
    existsWantedLockfile: files[0] != null,
    wantedLockfile,
    lockfileHadConflicts,
  }
}
