import logger from '@pnpm/logger'
import { IncludedDependencies } from '@pnpm/modules-yaml'
import { LocalPackages } from '@pnpm/resolver-base'
import { ReadPackageHook } from '@pnpm/types'
import normalizeRegistryUrl = require('normalize-registry-url')
import { StoreController } from 'package-store'
import path = require('path')
import { Shrinkwrap } from 'pnpm-shrinkwrap'
import pnpmPkgJson from '../pnpmPkgJson'
import { ReporterFunction } from '../types'

export interface InstallOptions {
  allowNew?: boolean,
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
  shrinkwrapDirectory?: string,
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
    afterAllResolved?: (shr: Shrinkwrap) => Shrinkwrap,
  },
  saveExact?: boolean,
  savePrefix?: string,
  saveProd?: boolean,
  saveDev?: boolean,
  saveOptional?: boolean,
  shamefullyFlatten?: boolean,
  sideEffectsCache?: boolean,
  sideEffectsCacheReadonly?: boolean,
  strictPeerDependencies?: boolean,
  bin?: string,
  include?: IncludedDependencies,
  independentLeaves?: boolean,
  ignoreCurrentPrefs?: boolean,
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
  ownLifecycleHooksStdio?: 'inherit' | 'pipe',
  localPackages?: LocalPackages,
}

export type StrictInstallOptions = InstallOptions & {
  allowNew: boolean,
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
  saveProd: boolean,
  saveDev: boolean,
  saveOptional: boolean,
  shamefullyFlatten: boolean,
  sideEffectsCache: boolean,
  sideEffectsCacheReadonly: boolean,
  strictPeerDependencies: boolean,
  bin: string,
  include: IncludedDependencies,
  independentLeaves: boolean,
  ignoreCurrentPrefs: boolean,
  ignoreScripts: boolean,
  childConcurrency: number,
  userAgent: string,
  lock: boolean,
  registry: string,
  lockStaleDuration: number,
  tag: string,
  locks: string,
  unsafePerm: boolean,
  ownLifecycleHooksStdio: 'inherit' | 'pipe',
  localPackages: LocalPackages,
}

const defaults = async (opts: InstallOptions) => {
  const packageManager = opts.packageManager || {
    name: pnpmPkgJson.name,
    version: pnpmPkgJson.version,
  }
  const prefix = opts.prefix || process.cwd()
  return {
    allowNew: true,
    bin: path.join(prefix, 'node_modules', '.bin'),
    childConcurrency: 5,
    depth: 0,
    engineStrict: false,
    force: false,
    frozenShrinkwrap: false,
    hooks: {},
    ignoreCurrentPrefs: false,
    ignoreScripts: false,
    include: {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    },
    independentLeaves: false,
    localPackages: {},
    lock: true,
    lockStaleDuration: 5 * 60 * 1000, // 5 minutes
    locks: path.join(opts.store, '_locks'),
    nodeVersion: process.version,
    ownLifecycleHooksStdio: 'inherit',
    packageManager,
    preferFrozenShrinkwrap: true,
    prefix,
    rawNpmConfig: {},
    registry: 'https://registry.npmjs.org/',
    repeatInstallDepth: -1,
    saveDev: false,
    saveExact: false,
    saveOptional: false,
    savePrefix: '^',
    saveProd: false,
    shamefullyFlatten: false,
    shrinkwrap: true,
    shrinkwrapOnly: false,
    sideEffectsCache: false,
    sideEffectsCacheReadonly: false,
    store: opts.store,
    storeController: opts.storeController,
    strictPeerDependencies: false,
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
    if (extendedOpts.shamefullyFlatten) {
      logger.info({
        message: 'Installing a flat node_modules. Use flat node_modules only if you rely on buggy dependencies that you cannot fix.',
        prefix: extendedOpts.prefix,
      })
    }
    if (!extendedOpts.shrinkwrap && extendedOpts.shrinkwrapOnly) {
      throw new Error('Cannot generate a shrinkwrap.yaml because shrinkwrap is set to false')
    }
    if (extendedOpts.userAgent.startsWith('npm/')) {
      extendedOpts.userAgent = `${extendedOpts.packageManager.name}/${extendedOpts.packageManager.version} ${extendedOpts.userAgent}`
    }
    extendedOpts.registry = normalizeRegistryUrl(extendedOpts.registry)
    extendedOpts.rawNpmConfig['registry'] = extendedOpts.registry // tslint:disable-line:no-string-literal
    // if sideEffectsCacheReadonly is true, sideEffectsCache is necessarily true too
    if (extendedOpts.sideEffectsCache && extendedOpts.sideEffectsCacheReadonly) {
      logger.warn({
        message: "--side-effects-cache-readonly turns on side effects cache too, you don't need to specify both",
        prefix: extendedOpts.prefix,
      })
    }
    extendedOpts.sideEffectsCache = extendedOpts.sideEffectsCache || extendedOpts.sideEffectsCacheReadonly
  }
  return extendedOpts
}
