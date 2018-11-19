import logger from '@pnpm/logger'
import {
  create as createShrinkwrap,
  existsWanted as existsWantedShrinkwrap,
  readCurrent as readCurrentShrinkwrap,
  readWanted as readWantedShrinkwrap,
  Shrinkwrap,
} from '@pnpm/shrinkwrap-file'
import isCI = require('is-ci')
import R = require('ramda')
import {
  SHRINKWRAP_NEXT_VERSION,
  SHRINKWRAP_VERSION,
} from '../constants'

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
    shrinkwrapDirectory: string,
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
  let shrinkwrapVersion
  if (opts.forceSharedShrinkwrap || opts.importers.length > 1 || opts.importers[0] && opts.importers[0].prefix !== opts.shrinkwrapDirectory) {
    shrinkwrapVersion = SHRINKWRAP_NEXT_VERSION
  } else {
    shrinkwrapVersion = SHRINKWRAP_VERSION
  }
  // ignore `shrinkwrap.yaml` on CI servers
  // a latest pnpm should not break all the builds
  const shrOpts = {
    ignoreIncompatible: opts.force || isCI,
    wantedVersion: shrinkwrapVersion,
  }
  const files = await Promise.all<Shrinkwrap | null | void>([
    opts.shrinkwrap && readWantedShrinkwrap(opts.shrinkwrapDirectory, shrOpts)
      || await existsWantedShrinkwrap(opts.shrinkwrapDirectory) &&
        logger.warn({
          message: 'A shrinkwrap.yaml file exists. The current configuration prohibits to read or write a shrinkwrap file',
          prefix: opts.shrinkwrapDirectory,
        }),
    readCurrentShrinkwrap(opts.shrinkwrapDirectory, shrOpts),
  ])
  const sopts = { shrinkwrapVersion }
  const importerIds = opts.importers.map((importer) => importer.id)
  const currentShrinkwrap = files[1] || createShrinkwrap(opts.registry, importerIds, sopts)
  for (const importerId of importerIds) {
    if (!currentShrinkwrap.importers[importerId]) {
      currentShrinkwrap.importers[importerId] = {
        specifiers: {},
      }
    }
  }
  const wantedShrinkwrap = files[0] ||
    currentShrinkwrap && R.clone(currentShrinkwrap) ||
    createShrinkwrap(opts.registry, importerIds, sopts)
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
