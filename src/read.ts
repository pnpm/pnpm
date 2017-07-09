import path = require('path')
import logger from './logger'
import {
  SHRINKWRAP_FILENAME,
  PRIVATE_SHRINKWRAP_FILENAME,
} from './constants'
import {Shrinkwrap} from './types'
import loadYamlFile = require('load-yaml-file')

const SHRINKWRAP_VERSION = 3

// TODO: move to separate package
type PnpmErrorCode = 'SHRINKWRAP_BREAKING_CHANGE'

class PnpmError extends Error {
  constructor (code: PnpmErrorCode, message: string) {
    super(message)
    this.code = code
  }
  code: PnpmErrorCode
}

class ShrinkwrapBreakingChangeError extends PnpmError {
  constructor (filename: string) {
    super('SHRINKWRAP_BREAKING_CHANGE', `Shrinkwrap file ${filename} not compatible with current pnpm`)
    this.filename = filename
  }
  filename: string
}

export async function readPrivate (
  pkgPath: string,
  opts: {
    ignoreIncompatible: boolean,
    registry: string,
  }
): Promise<Shrinkwrap> {
  const shrinkwrapPath = path.join(pkgPath, PRIVATE_SHRINKWRAP_FILENAME)
  return await _read(shrinkwrapPath, opts)
}

export async function read (
  pkgPath: string,
  opts: {
    ignoreIncompatible: boolean,
    registry: string,
}): Promise<Shrinkwrap> {
  const shrinkwrapPath = path.join(pkgPath, SHRINKWRAP_FILENAME)
  return await _read(shrinkwrapPath, opts)
}

async function _read (
  shrinkwrapPath: string,
  opts: {
    ignoreIncompatible: boolean,
    registry: string,
}): Promise<Shrinkwrap> {
  let shrinkwrap
  try {
    shrinkwrap = await loadYamlFile<Shrinkwrap>(shrinkwrapPath)
  } catch (err) {
    if ((<NodeJS.ErrnoException>err).code !== 'ENOENT') {
      throw err
    }
    return getDefaultShrinkwrap(opts.registry)
  }
  // for backward compatibility
  if (shrinkwrap && shrinkwrap['version'] === SHRINKWRAP_VERSION) {
    shrinkwrap.shrinkwrapVersion = SHRINKWRAP_VERSION
    delete shrinkwrap['version']
    return shrinkwrap
  }
  if (shrinkwrap && shrinkwrap.shrinkwrapVersion === SHRINKWRAP_VERSION) {
    return shrinkwrap
  }
  if (opts.ignoreIncompatible) {
    logger.warn(`Ignoring not compatible shrinkwrap file at ${shrinkwrapPath}`)
    return getDefaultShrinkwrap(opts.registry)
  }
  throw new ShrinkwrapBreakingChangeError(shrinkwrapPath)
}

function getDefaultShrinkwrap (registry: string) {
  return {
    shrinkwrapVersion: SHRINKWRAP_VERSION,
    specifiers: {},
    dependencies: {},
    packages: {},
    registry,
  }
}
