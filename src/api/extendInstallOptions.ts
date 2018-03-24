import logger from '@pnpm/logger'
import { ReadPackageHook } from '@pnpm/types'
import normalizeRegistryUrl = require('normalize-registry-url')
import {StoreController} from 'package-store'
import path = require('path')
import {LAYOUT_VERSION} from '../constants'
import pnpmPkgJson from '../pnpmPkgJson'
import { ReporterFunction } from '../types'

export interface InstallOptions {
  frozenShrinkwrap?: boolean,
  preferFrozenShrinkwrap?: boolean,
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
  frozenShrinkwrap: boolean,
  preferFrozenShrinkwrap: boolean,
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
  unsafePerm: boolean,
}

const defaults = async (opts: InstallOptions) => {
  const packageManager = opts.packageManager || {
    name: pnpmPkgJson.name,
    version: pnpmPkgJson.version,
  }
  const prefix = opts.prefix || process.cwd()
  return {
    bin: path.join(prefix, 'node_modules', '.bin'),
    childConcurrency: 5,
    depth: 0,
    development: true,
    engineStrict: false,
    force: false,
    frozenShrinkwrap: false,
    global: false,
    hooks: {},
    ignoreScripts: false,
    independentLeaves: false,
    lock: true,
    lockStaleDuration: 60 * 1000, // 1 minute
    locks: path.join(opts.store, '_locks'),
    nodeVersion: process.version,
    optional: typeof opts.production === 'boolean' ? opts.production : true,
    packageManager,
    preferFrozenShrinkwrap: false,
    prefix,
    production: true,
    rawNpmConfig: {},
    registry: 'https://registry.npmjs.org/',
    repeatInstallDepth: -1,
    saveDev: false,
    saveExact: false,
    saveOptional: false,
    savePrefix: '^',
    shamefullyFlatten: false,
    shrinkwrap: true,
    shrinkwrapOnly: false,
    sideEffectsCache: false,
    sideEffectsCacheReadonly: false,
    store: opts.store,
    storeController: opts.storeController,
    tag: 'latest',
    unsafePerm: process.platform === 'win32' ||
      process.platform === 'cygwin' ||
      !(process.getuid && process.setuid &&
        process.getgid && process.setgid) ||
      process.getuid() !== 0,
    update: false,
    userAgent: `${packageManager.name}/${packageManager.version} npm/? node/${process.version} ${process.platform} ${process.arch}`,
    verifyStoreIntegrity: true,
  } as StrictInstallOptions
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
  if (!extendedOpts.reinstallForFlatten) {
    if (extendedOpts.force) {
      logger.warn('using --force I sure hope you know what you are doing')
    }
    if (extendedOpts.lock === false) {
      logger.warn('using --no-lock I sure hope you know what you are doing')
    }
    if (extendedOpts.shamefullyFlatten) {
      logger.warn('using --shamefully-flatten is discouraged, you should declare all of your dependencies in package.json')
    }
    if (!extendedOpts.shrinkwrap && extendedOpts.shrinkwrapOnly) {
      throw new Error('Cannot generate a shrinkwrap.yaml because shrinkwrap is set to false')
    }
    if (extendedOpts.userAgent.startsWith('npm/')) {
      extendedOpts.userAgent = `${extendedOpts.packageManager.name}/${extendedOpts.packageManager.version} ${extendedOpts.userAgent}`
    }
    extendedOpts.registry = normalizeRegistryUrl(extendedOpts.registry)
    if (extendedOpts.global) {
      const independentLeavesSuffix = extendedOpts.independentLeaves ? '_independent_leaves' : ''
      const shamefullyFlattenSuffix = extendedOpts.shamefullyFlatten ? '_shamefully_flatten' : ''
      const subfolder = LAYOUT_VERSION.toString() + independentLeavesSuffix + shamefullyFlattenSuffix
      extendedOpts.prefix = path.join(extendedOpts.prefix, subfolder)
    }
    extendedOpts.rawNpmConfig['registry'] = extendedOpts.registry // tslint:disable-line:no-string-literal
    // if sideEffectsCacheReadonly is true, sideEffectsCache is necessarily true too
    if (extendedOpts.sideEffectsCache && extendedOpts.sideEffectsCacheReadonly) {
      logger.warn("--side-effects-cache-readonly turns on side effects cache too, you don't need to specify both")
    }
    extendedOpts.sideEffectsCache = extendedOpts.sideEffectsCache || extendedOpts.sideEffectsCacheReadonly
  }
  return extendedOpts
}
