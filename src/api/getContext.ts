import path = require('path')
import isCI = require('is-ci')
import {fromDir as readPkgFromDir} from '../fs/readPkg'
import writePkg = require('write-pkg')
import createStore, {
  StoreController,
} from 'package-store'
import {StrictSupiOptions} from '../types'
import {
  readWanted as readWantedShrinkwrap,
  readCurrent as readCurrentShrinkwrap,
  Shrinkwrap,
  create as createShrinkwrap,
} from 'pnpm-shrinkwrap'
import {
  read as readModules,
} from '../fs/modulesController'
import mkdirp = require('mkdirp-promise')
import {PackageJson} from '@pnpm/types'
import normalizePath = require('normalize-path')
import removeAllExceptOuterLinks = require('remove-all-except-outer-links')
import logger from '@pnpm/logger'
import checkCompatibility from './checkCompatibility'
import {packageJsonLogger} from '../loggers'
import createFetcher from '@pnpm/default-fetcher'
import createResolver from '@pnpm/default-resolver'

export type PnpmContext = {
  pkg: PackageJson,
  storeController: StoreController,
  storePath: string,
  root: string,
  existsWantedShrinkwrap: boolean,
  existsCurrentShrinkwrap: boolean,
  currentShrinkwrap: Shrinkwrap,
  wantedShrinkwrap: Shrinkwrap,
  skipped: Set<string>,
  pendingBuilds: string[],
}

export default async function getContext (opts: StrictSupiOptions, installType?: 'named' | 'general'): Promise<PnpmContext> {
  const root = normalizePath(opts.prefix)
  const storePath = opts.store

  const modulesPath = path.join(root, 'node_modules')
  const modules = await readModules(modulesPath)

  if (modules) {
    try {
      if (Boolean(modules.independentLeaves) !== opts.independentLeaves) {
        if (modules.independentLeaves) {
          throw new Error(`This node_modules was installed with --independent-leaves option.
            Use this option or run same command with --force to recreated node_modules`)
        }
        throw new Error(`This node_modules was not installed with the --independent-leaves option.
          Don't use --independent-leaves run same command with --force to recreated node_modules`)
      }
      checkCompatibility(modules, {storePath, modulesPath})
    } catch (err) {
      if (!opts.force) throw err
      if (installType !== 'general') {
        throw new Error('Named installation cannot be used to regenerate the node_modules structure. Run pnpm install --force')
      }
      logger.info(`Recreating ${modulesPath}`)
      await removeAllExceptOuterLinks(modulesPath)
      return getContext(opts)
    }
  }

  // ignore `shrinkwrap.yaml` on CI servers
  // a latest pnpm should not break all the builds
  const shrOpts = {ignoreIncompatible: opts.force || isCI}
  const files = await Promise.all([
    (opts.global ? readGlobalPkgJson(opts.prefix) : readPkgFromDir(opts.prefix)),
    readWantedShrinkwrap(root, shrOpts),
    readCurrentShrinkwrap(root, shrOpts),
    opts.storeController
      ? Promise.resolve(opts.storeController)
      : createStore(
        createResolver(opts),
        createFetcher(opts) as {},
        {
          networkConcurrency: opts.networkConcurrency,
          store: opts.store,
          locks: opts.lock ? opts.locks : undefined,
          lockStaleDuration: opts.lockStaleDuration,
        }
      ),
    mkdirp(storePath),
  ])
  const ctx: PnpmContext = {
    pkg: files[0],
    root,
    storePath,
    wantedShrinkwrap: files[1] || createShrinkwrap(opts.registry),
    currentShrinkwrap: files[2] || createShrinkwrap(opts.registry),
    existsWantedShrinkwrap: !!files[1],
    existsCurrentShrinkwrap: !!files[2],
    storeController: files[3],
    skipped: new Set(modules && modules.skipped || []),
    pendingBuilds: modules && modules.pendingBuilds || [],
  }
  packageJsonLogger.debug({ initial: ctx.pkg })

  return ctx
}

const DefaultGlobalPkg: PackageJson = {
  name: 'pnpm-global-pkg',
  version: '1.0.0',
  private: true,
}

async function readGlobalPkgJson (globalPkgPath: string) {
  try {
    const globalPkgJson = await readPkgFromDir(globalPkgPath)
    return globalPkgJson
  } catch (err) {
    await writePkg(globalPkgPath, DefaultGlobalPkg)
    return DefaultGlobalPkg
  }
}
