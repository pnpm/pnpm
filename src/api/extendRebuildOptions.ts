import path = require('path')
import logger from '@pnpm/logger'
import pnpmPkgJson from '../pnpmPkgJson'
import {LAYOUT_VERSION} from '../fs/modulesController'
import normalizeRegistryUrl = require('normalize-registry-url')
import {StoreController} from 'package-store'
import { ReporterFunction } from '../types'

export type RebuildOptions = {
  prefix?: string,
  store: string, // TODO: remove this property
  independentLeaves?: boolean,
  force?: boolean,
  global?: boolean,
  registry?: string,
  shrinkwrap?: boolean,

  reporter?: ReporterFunction,
  production?: boolean,
  development?: boolean,
  optional?: boolean,
  bin?: string,
  rawNpmConfig?: object,
  userAgent?: string,
  packageManager?: {
    name: string,
    version: string,
  },
  unsafePerm?: boolean,
  pending?: boolean,
  shamefullyFlatten?: boolean,
}

export type StrictRebuildOptions = RebuildOptions & {
  prefix: string,
  store: string,
  independentLeaves: boolean,
  force: boolean,
  global: boolean,
  registry: string,
  bin: string,
  rawNpmConfig: object,
  userAgent: string,
  packageManager: {
    name: string,
    version: string,
  },
  unsafePerm: boolean,
  pending: boolean,
  shrinkwrap: boolean,
  shamefullyFlatten: boolean,
}

const defaults = async (opts: RebuildOptions) => {
  const packageManager = opts.packageManager || {
    name: pnpmPkgJson.name,
    version: pnpmPkgJson.version,
  }
  const prefix = opts.prefix || process.cwd()
  return <StrictRebuildOptions>{
    pending: false,
    global: false,
    store: opts.store,
    bin: path.join(prefix, 'node_modules', '.bin'),
    userAgent: `${packageManager.name}/${packageManager.version} npm/? node/${process.version} ${process.platform} ${process.arch}`,
    packageManager,
    prefix,
    force: false,
    registry: 'https://registry.npmjs.org/',
    rawNpmConfig: {},
    independentLeaves: false,
    unsafePerm: process.platform === 'win32' ||
                process.platform === 'cygwin' ||
                !(process.getuid && process.setuid &&
                  process.getgid && process.setgid) ||
                process.getuid() !== 0,
    shrinkwrap: true,
    shamefullyFlatten: false,
  }
}

export default async (
  opts: RebuildOptions,
): Promise<StrictRebuildOptions> => {
  if (opts) {
    for (const key in opts) {
      if (opts[key] === undefined) {
        delete opts[key]
      }
    }
  }
  const defaultOpts = await defaults(opts)
  const extendedOpts = {...defaultOpts, ...opts, store: defaultOpts.store}
  if (extendedOpts.force) {
    logger.warn('using --force I sure hope you know what you are doing')
  }
  extendedOpts.registry = normalizeRegistryUrl(extendedOpts.registry)
  if (extendedOpts.global) {
    const subfolder = LAYOUT_VERSION.toString() + (extendedOpts.independentLeaves ? '_independent_leaves' : '')
    extendedOpts.prefix = path.join(extendedOpts.prefix, subfolder)
  }
  return extendedOpts
}
