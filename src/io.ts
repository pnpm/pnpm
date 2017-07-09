import path = require('path')
import loadYamlFile = require('load-yaml-file')
import rimraf = require('rimraf-then')
import {Shrinkwrap} from './types'
import logger from 'pnpm-logger'
import yaml = require('js-yaml')
import writeFileAtomicCB = require('write-file-atomic')
import thenify = require('thenify')

const writeFileAtomic = thenify(writeFileAtomicCB)

const shrinkwrapLogger = logger('shrinkwrap')

export const SHRINKWRAP_FILENAME = 'shrinkwrap.yaml'
export const PRIVATE_SHRINKWRAP_FILENAME = path.join('node_modules', '.shrinkwrap.yaml')
const SHRINKWRAP_VERSION = 3

const SHRINKWRAP_YAML_FORMAT = {
  sortKeys: true,
  lineWidth: 1000,
  noCompatMode: true,
}

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

function getDefaultShrinkwrap (registry: string) {
  return {
    shrinkwrapVersion: SHRINKWRAP_VERSION,
    specifiers: {},
    dependencies: {},
    packages: {},
    registry,
  }
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
    shrinkwrapLogger.warn(`Ignoring not compatible shrinkwrap file at ${shrinkwrapPath}`)
    return getDefaultShrinkwrap(opts.registry)
  }
  throw new ShrinkwrapBreakingChangeError(shrinkwrapPath)
}

export function save (pkgPath: string, shrinkwrap: Shrinkwrap) {
  const shrinkwrapPath = path.join(pkgPath, SHRINKWRAP_FILENAME)
  const privateShrinkwrapPath = path.join(pkgPath, PRIVATE_SHRINKWRAP_FILENAME)

  // empty shrinkwrap is not saved
  if (Object.keys(shrinkwrap.specifiers).length === 0) {
    return Promise.all([
      rimraf(shrinkwrapPath),
      rimraf(privateShrinkwrapPath),
    ])
  }

  const yamlDoc = yaml.safeDump(shrinkwrap, SHRINKWRAP_YAML_FORMAT)

  return Promise.all([
    writeFileAtomic(shrinkwrapPath, yamlDoc),
    writeFileAtomic(privateShrinkwrapPath, yamlDoc),
  ])
}
