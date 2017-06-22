import path = require('path')
import {fromDir as readPkgFromDir} from '../fs/readPkg'
import writePkg = require('write-pkg')
import expandTilde, {isHomepath} from '../fs/expandTilde'
import {StrictPnpmOptions} from '../types'
import {
  read as readShrinkwrap,
  readPrivate as readPrivateShrinkwrap,
  Shrinkwrap,
} from 'pnpm-lockfile'
import {
  read as readModules,
} from '../fs/modulesController'
import mkdirp = require('mkdirp-promise')
import {Package} from '../types'
import normalizePath = require('normalize-path')
import removeAllExceptOuterLinks = require('remove-all-except-outer-links')
import logger from 'pnpm-logger'
import checkCompatibility from './checkCompatibility'

const STORE_VERSION = '2'

export type PnpmContext = {
  pkg: Package,
  storePath: string,
  root: string,
  privateShrinkwrap: Shrinkwrap,
  shrinkwrap: Shrinkwrap,
  skipped: Set<string>,
}

export default async function getContext (opts: StrictPnpmOptions, installType?: 'named' | 'general'): Promise<PnpmContext> {
  const pkg = await (opts.global ? readGlobalPkgJson(opts.prefix) : readPkgFromDir(opts.prefix))
  const root = normalizePath(opts.prefix)
  const storeBasePath = resolveStoreBasePath(opts.store, root)

  const storePath = path.join(storeBasePath, STORE_VERSION)

  const modulesPath = path.join(root, 'node_modules')
  let modules = await readModules(modulesPath)

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

  const shrinkwrap = await readShrinkwrap(root, {force: opts.force, registry: opts.registry})
  const ctx: PnpmContext = {
    pkg,
    root,
    storePath,
    shrinkwrap,
    privateShrinkwrap: await readPrivateShrinkwrap(root, {force: opts.force, registry: opts.registry}),
    skipped: new Set(modules && modules.skipped || []),
  }

  await mkdirp(ctx.storePath)
  return ctx
}

const DefaultGlobalPkg: Package = {
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

function resolveStoreBasePath (storePath: string, pkgRoot: string) {
  if (isHomepath(storePath)) {
    return expandTilde(storePath)
  }
  return path.resolve(pkgRoot, storePath)
}
