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
} from '@pnpm/lockfile-file'
import logger from '@pnpm/logger'
import isCI = require('is-ci')
import R = require('ramda')

export interface PnpmContext {
  currentLockfile: Lockfile,
  existsCurrentLockfile: boolean,
  existsWantedLockfile: boolean,
  wantedLockfile: Lockfile,
}

export default async function (
  opts: {
    force: boolean,
    forceSharedLockfile: boolean,
    importers: Array<{
      id: string,
      prefix: string,
    }>,
    lockfileDirectory: string,
    registry: string,
    useLockfile: boolean,
    virtualStoreDir: string,
  },
): Promise<{
  currentLockfile: Lockfile,
  existsCurrentLockfile: boolean,
  existsWantedLockfile: boolean,
  wantedLockfile: Lockfile,
}> {
  // ignore `pnpm-lock.yaml` on CI servers
  // a latest pnpm should not break all the builds
  const lockfileOpts = {
    ignoreIncompatible: opts.force || isCI,
    wantedVersion: LOCKFILE_VERSION,
  }
  const files = await Promise.all<Lockfile | null | void>([
    opts.useLockfile && readWantedLockfile(opts.lockfileDirectory, lockfileOpts)
      || await existsWantedLockfile(opts.lockfileDirectory) &&
        logger.warn({
          message: `A ${WANTED_LOCKFILE} file exists. The current configuration prohibits to read or write a lockfile`,
          prefix: opts.lockfileDirectory,
        }),
    readCurrentLockfile(opts.virtualStoreDir, lockfileOpts),
  ])
  const sopts = { lockfileVersion: LOCKFILE_VERSION }
  const importerIds = opts.importers.map((importer) => importer.id)
  const currentLockfile = files[1] || createLockfileObject(importerIds, sopts)
  for (const importerId of importerIds) {
    if (!currentLockfile.importers[importerId]) {
      currentLockfile.importers[importerId] = {
        specifiers: {},
      }
    }
  }
  const wantedLockfile = files[0] ||
    currentLockfile && R.clone(currentLockfile) ||
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
    existsCurrentLockfile: !!files[1],
    existsWantedLockfile: !!files[0],
    wantedLockfile,
  }
}
