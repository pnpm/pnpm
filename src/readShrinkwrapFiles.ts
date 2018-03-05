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

export interface PnpmContext {
  currentShrinkwrap: Shrinkwrap,
  existsCurrentShrinkwrap: boolean,
  existsWantedShrinkwrap: boolean,
  wantedShrinkwrap: Shrinkwrap,
}

export default async function getContext (
  opts: {
    force: boolean,
    prefix: string,
    registry: string,
    shrinkwrap: boolean,
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
    opts.shrinkwrap && readWantedShrinkwrap(opts.prefix, shrOpts)
      || await existsWantedShrinkwrap(opts.prefix) &&
        logger.warn('A shrinkwrap.yaml file exists. The current configuration prohibits to read or write a shrinkwrap file'),
    readCurrentShrinkwrap(opts.prefix, shrOpts),
  ])
  const currentShrinkwrap = files[1] || createShrinkwrap(opts.registry)
  return {
    currentShrinkwrap,
    existsCurrentShrinkwrap: !!files[1],
    existsWantedShrinkwrap: !!files[0],
    wantedShrinkwrap: files[0] || !opts.shrinkwrap && currentShrinkwrap && R.clone(currentShrinkwrap) || createShrinkwrap(opts.registry),
  }
}
