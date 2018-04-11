import logger from '@pnpm/logger'
import normalizeRegistryUrl = require('normalize-registry-url')
import {StoreController} from 'package-store'
import path = require('path')
import {LAYOUT_VERSION} from '../constants'
import pnpmPkgJson from '../pnpmPkgJson'
import { ReporterFunction } from '../types'

export interface RebuildOptions {
  childConcurrency?: number,
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
  childConcurrency?: number,
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
  return {
    bin: path.join(prefix, 'node_modules', '.bin'),
    childConcurrency: 5,
    development: true,
    force: false,
    global: false,
    independentLeaves: false,
    optional: true,
    packageManager,
    pending: false,
    prefix,
    production: true,
    rawNpmConfig: {},
    registry: 'https://registry.npmjs.org/',
    shamefullyFlatten: false,
    shrinkwrap: true,
    store: opts.store,
    unsafePerm: process.platform === 'win32' ||
      process.platform === 'cygwin' ||
      !(process.getuid && process.setuid &&
        process.getgid && process.setgid) ||
      process.getuid() !== 0,
    userAgent: `${packageManager.name}/${packageManager.version} npm/? node/${process.version} ${process.platform} ${process.arch}`,
  } as StrictRebuildOptions
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
