import logger from '@pnpm/logger'
import isCI = require('is-ci')
import {
  create as createShrinkwrap,
  existsWanted as existsWantedShrinkwrap,
  readCurrent as readCurrentShrinkwrap,
  readWanted as readWantedShrinkwrap,
  Shrinkwrap,
} from 'pnpm-shrinkwrap'
import R = require('ramda')
import { SHRINKWRAP_MINOR_VERSION } from '../constants'

export interface PnpmContext {
  currentShrinkwrap: Shrinkwrap,
  existsCurrentShrinkwrap: boolean,
  existsWantedShrinkwrap: boolean,
  wantedShrinkwrap: Shrinkwrap,
}

export default async function (
  opts: {
    force: boolean,
    shrinkwrapDirectory: string,
    registry: string,
    shrinkwrap: boolean,
    importerPaths: string[],
  },
): Promise<{
  currentShrinkwrap: Shrinkwrap,
  existsCurrentShrinkwrap: boolean,
  existsWantedShrinkwrap: boolean,
  wantedShrinkwrap: Shrinkwrap,
}> {
  // ignore `shrinkwrap.yaml` on CI servers
  // a latest pnpm should not break all the builds
  const shrOpts = {ignoreIncompatible: opts.force || isCI}
  const files = await Promise.all<Shrinkwrap | null | void>([
    opts.shrinkwrap && readWantedShrinkwrap(opts.shrinkwrapDirectory, shrOpts)
      || await existsWantedShrinkwrap(opts.shrinkwrapDirectory) &&
        logger.warn({
          message: 'A shrinkwrap.yaml file exists. The current configuration prohibits to read or write a shrinkwrap file',
          prefix: opts.shrinkwrapDirectory,
        }),
    readCurrentShrinkwrap(opts.shrinkwrapDirectory, shrOpts),
  ])
  const sopts = { shrinkwrapMinorVersion: SHRINKWRAP_MINOR_VERSION }
  const currentShrinkwrap = files[1] || createShrinkwrap(opts.registry, opts.importerPaths, sopts)
  for (const importerPath of opts.importerPaths) {
    if (!currentShrinkwrap.importers[importerPath]) {
      currentShrinkwrap.importers[importerPath] = {
        specifiers: {},
      }
    }
  }
  const wantedShrinkwrap = files[0] ||
    !opts.shrinkwrap && currentShrinkwrap && R.clone(currentShrinkwrap) ||
    createShrinkwrap(opts.registry, opts.importerPaths, sopts)
  for (const importerPath of opts.importerPaths) {
    if (!wantedShrinkwrap.importers[importerPath]) {
      wantedShrinkwrap.importers[importerPath] = {
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
