import path = require('path')
import {Resolution} from '../resolve'
import {PnpmError} from '../errorTypes'
import logger from 'pnpm-logger'
import loadYamlFile = require('load-yaml-file')
import writeYamlFile = require('write-yaml-file')

const shrinkwrapLogger = logger('shrinkwrap')

const SHRINKWRAP_FILENAME = 'shrinkwrap.yaml'
const SHRINKWRAP_VERSION = 0

function getDefaultShrinkwrap () {
  return {
    version: SHRINKWRAP_VERSION,
    dependencies: {},
    packages: {},
  }
}

export type Shrinkwrap = {
  version: number,
  dependencies: ResolvedDependencies,
  packages: {
    [pkgId: string]: DependencyShrinkwrap,
  },
}

export type DependencyShrinkwrap = {
  resolution: Resolution,
  dependencies?: ResolvedDependencies,
}

/*** @example
 * {
 *   "foo": "registry.npmjs.org/foo/1.0.1"
 * }
 */
export type ResolvedDependencies = {
  [pkgName: string]: string,
}

export async function read (pkgPath: string, opts: {force: boolean}): Promise<Shrinkwrap> {
  const shrinkwrapPath = path.join(pkgPath, SHRINKWRAP_FILENAME)
  let shrinkwrap
  try {
    shrinkwrap = await loadYamlFile<Shrinkwrap>(shrinkwrapPath)
  } catch (err) {
    if ((<NodeJS.ErrnoException>err).code !== 'ENOENT') {
      throw err
    }
    return getDefaultShrinkwrap()
  }
  if (shrinkwrap && shrinkwrap.version === SHRINKWRAP_VERSION) {
    return shrinkwrap
  }
  if (opts.force) {
    shrinkwrapLogger.warn(`Ignoring not compatible shrinkwrap file at ${shrinkwrapPath}`)
    return getDefaultShrinkwrap()
  }
  throw new ShrinkwrapBreakingChangeError(shrinkwrapPath)
}

export function save (pkgPath: string, shrinkwrap: Shrinkwrap) {
  const shrinkwrapPath = path.join(pkgPath, SHRINKWRAP_FILENAME)
  return writeYamlFile(shrinkwrapPath, shrinkwrap, {
    sortKeys: true,
    lineWidth: 1000,
    noCompatMode: true,
  })
}

class ShrinkwrapBreakingChangeError extends PnpmError {
  constructor (pathToShrinkwrap: string) {
    super('SHRINKWRAP_BREAKING_CHANGE', `Shrinkwrap file ${pathToShrinkwrap} not compatible with current pnpm`)
    this.pathToShrinkwrap = pathToShrinkwrap
  }
  pathToShrinkwrap: string
}
