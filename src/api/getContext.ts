import logger from '@pnpm/logger'
import {PackageJson} from '@pnpm/types'
import mkdirp = require('mkdirp-promise')
import normalizePath = require('normalize-path')
import path = require('path')
import {Shrinkwrap} from 'pnpm-shrinkwrap'
import removeAllExceptOuterLinks = require('remove-all-except-outer-links')
import writePkg = require('write-pkg')
import {
  read as readModules,
} from '../fs/modulesController'
import {fromDir as safeReadPkgFromDir} from '../fs/safeReadPkg'
import {packageJsonLogger} from '../loggers'
import readShrinkwrapFile from '../readShrinkwrapFiles'
import {StrictSupiOptions} from '../types'
import checkCompatibility from './checkCompatibility'

export interface PnpmContext {
  pkg: PackageJson,
  storePath: string,
  root: string,
  existsWantedShrinkwrap: boolean,
  existsCurrentShrinkwrap: boolean,
  currentShrinkwrap: Shrinkwrap,
  wantedShrinkwrap: Shrinkwrap,
  skipped: Set<string>,
  pendingBuilds: string[],
  hoistedAliases: {[pkgId: string]: string[]}
}

export default async function getContext (
  opts: {
    prefix: string,
    shamefullyFlatten: boolean,
    shrinkwrap: boolean,
    store: string,
    independentLeaves: boolean,
    force: boolean,
    global: boolean,
    registry: string,
  },
  installType?: 'named' | 'general',
): Promise<PnpmContext> {
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
      if (Boolean(modules.shamefullyFlatten) !== opts.shamefullyFlatten) {
        if (modules.shamefullyFlatten) {
          throw new Error(`This node_modules was installed with --shamefully-flatten option.
            Use this option or run same command with --force to recreated node_modules`)
        }
        throw new Error(`This node_modules was not installed with the --shamefully-flatten option.
          Don't use --shamefully-flatten or run same command with --force to recreated node_modules`)
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

  const files = await Promise.all([
    (opts.global ? readGlobalPkgJson(opts.prefix) : safeReadPkgFromDir(opts.prefix)),
    mkdirp(storePath),
  ])
  const ctx: PnpmContext = {
    hoistedAliases: modules && modules.hoistedAliases || {},
    pendingBuilds: modules && modules.pendingBuilds || [],
    pkg: files[0] || {} as PackageJson,
    root,
    skipped: new Set(modules && modules.skipped || []),
    storePath,
    ...await readShrinkwrapFile(opts),
  }
  packageJsonLogger.debug({ initial: ctx.pkg })

  return ctx
}

const DefaultGlobalPkg: PackageJson = {
  name: 'pnpm-global-pkg',
  private: true,
  version: '1.0.0',
}

async function readGlobalPkgJson (globalPkgPath: string) {
  const globalPkgJson = await safeReadPkgFromDir(globalPkgPath)
  if (globalPkgJson) return globalPkgJson
  await writePkg(globalPkgPath, DefaultGlobalPkg)
  return DefaultGlobalPkg
}
