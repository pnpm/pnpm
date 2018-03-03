import {
  existsWanted as existsWantedShrinkwrap,
  readWanted as readWantedShrinkwrap,
  readCurrent as readCurrentShrinkwrap,
  Shrinkwrap,
  create as createShrinkwrap,
} from 'pnpm-shrinkwrap'
import logger from '@pnpm/logger'
import R = require('ramda')
import isCI = require('is-ci')

export type PnpmContext = {
  existsWantedShrinkwrap: boolean,
  existsCurrentShrinkwrap: boolean,
  currentShrinkwrap: Shrinkwrap,
  wantedShrinkwrap: Shrinkwrap,
}

export default async function getContext (
  opts: {
    prefix: string,
    shrinkwrap: boolean,
    force: boolean,
    registry: string,
  },
): Promise<{
  existsWantedShrinkwrap: boolean,
  existsCurrentShrinkwrap: boolean,
  currentShrinkwrap: Shrinkwrap,
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
    wantedShrinkwrap: files[0] || !opts.shrinkwrap && currentShrinkwrap && R.clone(currentShrinkwrap) || createShrinkwrap(opts.registry),
    currentShrinkwrap,
    existsWantedShrinkwrap: !!files[0],
    existsCurrentShrinkwrap: !!files[1],
  }
}
