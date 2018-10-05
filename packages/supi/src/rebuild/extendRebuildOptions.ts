import normalizeRegistryUrl = require('normalize-registry-url')
import path = require('path')
import pnpmPkgJson from '../pnpmPkgJson'
import { ReporterFunction } from '../types'

export interface RebuildOptions {
  childConcurrency?: number,
  prefix?: string,
  shrinkwrapDirectory?: string,
  store: string, // TODO: remove this property
  independentLeaves?: boolean,
  force?: boolean,
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
  shrinkwrapDirectory: string,
  independentLeaves: boolean,
  force: boolean,
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
  const shrinkwrapDirectory = opts.shrinkwrapDirectory || prefix
  return {
    bin: path.join(prefix, 'node_modules', '.bin'),
    childConcurrency: 5,
    development: true,
    force: false,
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
    shrinkwrapDirectory,
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
  extendedOpts.registry = normalizeRegistryUrl(extendedOpts.registry)
  return extendedOpts
}
