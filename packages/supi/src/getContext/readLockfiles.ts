import {
  SHRINKWRAP_VERSION,
  WANTED_LOCKFILE,
} from '@pnpm/constants'
import {
  createLockfileObject,
  existsWantedLockfile,
  readCurrentLockfile,
  readWantedLockfile,
  Shrinkwrap,
} from '@pnpm/lockfile-file'
import logger from '@pnpm/logger'
import isCI = require('is-ci')
import R = require('ramda')

export interface PnpmContext {
  currentShrinkwrap: Shrinkwrap,
  existsCurrentShrinkwrap: boolean,
  existsWantedShrinkwrap: boolean,
  wantedShrinkwrap: Shrinkwrap,
}

export default async function (
  opts: {
    force: boolean,
    forceSharedShrinkwrap: boolean,
    lockfileDirectory: string,
    registry: string,
    shrinkwrap: boolean,
    importers: Array<{
      id: string,
      prefix: string,
    }>,
  },
): Promise<{
  currentShrinkwrap: Shrinkwrap,
  existsCurrentShrinkwrap: boolean,
  existsWantedShrinkwrap: boolean,
  wantedShrinkwrap: Shrinkwrap,
}> {
  // ignore `shrinkwrap.yaml` on CI servers
  // a latest pnpm should not break all the builds
  const shrOpts = {
    ignoreIncompatible: opts.force || isCI,
    wantedVersion: SHRINKWRAP_VERSION,
  }
  const files = await Promise.all<Shrinkwrap | null | void>([
    opts.shrinkwrap && readWantedLockfile(opts.lockfileDirectory, shrOpts)
      || await existsWantedLockfile(opts.lockfileDirectory) &&
        logger.warn({
          message: `A ${WANTED_LOCKFILE} file exists. The current configuration prohibits to read or write a shrinkwrap file`,
          prefix: opts.lockfileDirectory,
        }),
    readCurrentLockfile(opts.lockfileDirectory, shrOpts),
  ])
  const sopts = { lockfileVersion: SHRINKWRAP_VERSION }
  const importerIds = opts.importers.map((importer) => importer.id)
  const currentShrinkwrap = files[1] || createLockfileObject(importerIds, sopts)
  for (const importerId of importerIds) {
    if (!currentShrinkwrap.importers[importerId]) {
      currentShrinkwrap.importers[importerId] = {
        specifiers: {},
      }
    }
  }
  const wantedShrinkwrap = files[0] ||
    currentShrinkwrap && R.clone(currentShrinkwrap) ||
    createLockfileObject(importerIds, sopts)
  for (const importerId of importerIds) {
    if (!wantedShrinkwrap.importers[importerId]) {
      wantedShrinkwrap.importers[importerId] = {
        specifiers: {},
      }
    }
  }
  return {
    currentShrinkwrap,
    existsCurrentShrinkwrap: !!files[1],
    existsWantedShrinkwrap: !!files[0],
    wantedShrinkwrap,
  }
}
