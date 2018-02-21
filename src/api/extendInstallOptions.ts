import path = require('path')
import logger from '@pnpm/logger'
import pnpmPkgJson from '../pnpmPkgJson'
import {LAYOUT_VERSION} from '../fs/modulesController'
import normalizeRegistryUrl = require('normalize-registry-url')
import {StoreController} from 'package-store'
import { ReporterFunction } from '../types'
import { ReadPackageHook } from '@pnpm/types'

export type InstallOptions = {
  storeController: StoreController,
  store: string,
  reporter?: ReporterFunction,
  shrinkwrap?: boolean,
  shrinkwrapOnly?: boolean,
  force?: boolean,
  update?: boolean,
  depth?: number,
  repeatInstallDepth?: number,
  prefix?: string,
  rawNpmConfig?: object,
  verifyStoreIntegrity?: boolean,
  engineStrict?: boolean,
  nodeVersion?: string,
  packageManager?: {
    name: string,
    version: string,
  },
  hooks?: {
    readPackage?: ReadPackageHook,
  },
  saveExact?: boolean,
  savePrefix?: string,
  saveDev?: boolean,
  saveOptional?: boolean,
  shamefullyFlatten?: boolean,
  sideEffectsCache?: boolean,
  sideEffectsCacheReadonly?: boolean,
  global?: boolean,
  bin?: string,
  production?: boolean,
  development?: boolean,
  optional?: boolean,
  independentLeaves?: boolean,
  ignoreScripts?: boolean,
  childConcurrency?: number,
  userAgent?: string,
  unsafePerm?: boolean,
  registry?: string,
  lock?: boolean,
  reinstallForFlatten?: boolean,
  lockStaleDuration?: number,
  tag?: string,
  locks?: string,
}

export type StrictInstallOptions = InstallOptions & {
  shrinkwrap: boolean,
  shrinkwrapOnly: boolean,
  force: boolean,
  update: boolean,
  prefix: string,
  depth: number,
  repeatInstallDepth: number,
  engineStrict: boolean,
  nodeVersion: string,
  rawNpmConfig: object,
  verifyStoreIntegrity: boolean,
  packageManager: {
    name: string,
    version: string,
  },
  hooks: {
    readPackage?: ReadPackageHook,
  },
  saveExact: boolean,
  savePrefix: string,
  saveDev: boolean,
  saveOptional: boolean,
  shamefullyFlatten: boolean,
  sideEffectsCache: boolean,
  sideEffectsCacheReadonly: boolean,
  global: boolean,
  bin: string,
  production: boolean,
  development: boolean,
  optional: boolean,
  independentLeaves: boolean,
  ignoreScripts: boolean,
  childConcurrency: number,
  userAgent: string,
  lock: boolean,
  registry: string,
  lockStaleDuration: number,
  tag: string,
  locks: string,
}

const defaults = async (opts: InstallOptions) => {
  const packageManager = opts.packageManager || {
    name: pnpmPkgJson.name,
    version: pnpmPkgJson.version,
  }
  const prefix = opts.prefix || process.cwd()
  return <StrictInstallOptions>{
    storeController: opts.storeController,
    shrinkwrap: true,
    shrinkwrapOnly: false,
    saveExact: false,
    global: false,
    store: opts.store,
    locks: path.join(opts.store, '_locks'),
    ignoreScripts: false,
    tag: 'latest',
    production: true,
    development: true,
    bin: path.join(prefix, 'node_modules', '.bin'),
    prefix,
    nodeVersion: process.version,
    force: false,
    depth: 0,
    engineStrict: false,
    lockStaleDuration: 60 * 1000, // 1 minute
    lock: true,
    childConcurrency: 5,
    registry: 'https://registry.npmjs.org/',
    userAgent: `${packageManager.name}/${packageManager.version} npm/? node/${process.version} ${process.platform} ${process.arch}`,
    rawNpmConfig: {},
    update: false,
    repeatInstallDepth: -1,
    optional: typeof opts.production === 'boolean' ? opts.production : true,
    independentLeaves: false,
    packageManager,
    verifyStoreIntegrity: true,
    hooks: {},
    savePrefix: '^',
    shamefullyFlatten: false,
    sideEffectsCache: false,
    sideEffectsCacheReadonly: false,
    unsafePerm: process.platform === 'win32' ||
                process.platform === 'cygwin' ||
                !(process.getuid && process.setuid &&
                  process.getgid && process.setgid) ||
                process.getuid() !== 0,
    saveDev: false,
    saveOptional: false,
  }
}

export default async (
  opts: InstallOptions,
): Promise<StrictInstallOptions> => {
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
  if (extendedOpts.lock === false && !extendedOpts.reinstallForFlatten) {
    logger.warn('using --no-lock I sure hope you know what you are doing')
  }
  if (!extendedOpts.shrinkwrap && extendedOpts.shrinkwrapOnly) {
    throw new Error('Cannot generate a shrinkwrap.yaml because shrinkwrap is set to false')
  }
  if (extendedOpts.userAgent.startsWith('npm/')) {
    extendedOpts.userAgent = `${extendedOpts.packageManager.name}/${extendedOpts.packageManager.version} ${extendedOpts.userAgent}`
  }
  extendedOpts.registry = normalizeRegistryUrl(extendedOpts.registry)
  if (extendedOpts.global) {
    const subfolder = LAYOUT_VERSION.toString() + (extendedOpts.independentLeaves ? '_independent_leaves' : '')
    extendedOpts.prefix = path.join(extendedOpts.prefix, subfolder)
  }
  extendedOpts.rawNpmConfig['registry'] = extendedOpts.registry
  // if sideEffectsCacheReadonly is true, sideEffectsCache is necessarily true too
  if (extendedOpts.sideEffectsCache && extendedOpts.sideEffectsCacheReadonly) {
    logger.warn("--side-effects-cache-readonly turns on side effects cache too, you don't need to specify both")
  }
  extendedOpts.sideEffectsCache = extendedOpts.sideEffectsCache || extendedOpts.sideEffectsCacheReadonly
  return extendedOpts
}
