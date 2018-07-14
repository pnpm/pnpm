import logger from '@pnpm/logger'
import {read as readModulesYaml} from '@pnpm/modules-yaml'
import {
  PackageJson,
  ReadPackageHook,
} from '@pnpm/types'
import {
  packageJsonLogger,
  safeReadPackageFromDir as safeReadPkgFromDir,
} from '@pnpm/utils'
import mkdirp = require('mkdirp-promise')
import path = require('path')
import {Shrinkwrap} from 'pnpm-shrinkwrap'
import removeAllExceptOuterLinks = require('remove-all-except-outer-links')
import writePkg = require('write-pkg')
import {PnpmError} from '../errorTypes'
import readShrinkwrapFile from '../readShrinkwrapFiles'
import {StrictSupiOptions} from '../types'
import checkCompatibility from './checkCompatibility'

export interface PnpmContext {
  pkg: PackageJson,
  storePath: string,
  prefix: string,
  existsWantedShrinkwrap: boolean,
  existsCurrentShrinkwrap: boolean,
  currentShrinkwrap: Shrinkwrap,
  wantedShrinkwrap: Shrinkwrap,
  skipped: Set<string>,
  pendingBuilds: string[],
  hoistedAliases: {[depPath: string]: string[]}
}

export default async function getContext (
  opts: {
    force: boolean,
    global: boolean,
    hooks?: {
      readPackage?: ReadPackageHook,
    },
    independentLeaves: boolean,
    prefix: string,
    registry: string,
    shamefullyFlatten: boolean,
    shrinkwrap: boolean,
    store: string,
  },
  installType?: 'named' | 'general',
): Promise<PnpmContext> {
  const storePath = opts.store

  const modulesPath = path.join(opts.prefix, 'node_modules')
  const modules = await readModulesYaml(modulesPath)

  if (modules) {
    try {
      if (Boolean(modules.independentLeaves) !== opts.independentLeaves) {
        if (modules.independentLeaves) {
          throw new PnpmError(
            'ERR_PNPM_INDEPENDENT_LEAVES_WANTED',
            `This node_modules was installed with --independent-leaves option.
            Use this option or run same command with --force to recreated node_modules`,
          )
        }
        throw new PnpmError(
          'ERR_PNPM_INDEPENDENT_LEAVES_NOT_WANTED',
          `This node_modules was not installed with the --independent-leaves option.
          Don't use --independent-leaves run same command with --force to recreated node_modules`,
        )
      }
      if (Boolean(modules.shamefullyFlatten) !== opts.shamefullyFlatten) {
        if (modules.shamefullyFlatten) {
          throw new PnpmError(
            'ERR_PNPM_SHAMEFULLY_FLATTEN_WANTED',
            `This node_modules was installed with --shamefully-flatten option.
            Use this option or run same command with --force to recreated node_modules`,
          )
        }
        throw new PnpmError(
          'ERR_PNPM_SHAMEFULLY_FLATTEN_NOT_WANTED',
          `This node_modules was not installed with the --shamefully-flatten option.
          Don't use --shamefully-flatten or run same command with --force to recreated node_modules`,
        )
      }
      checkCompatibility(modules, {storePath, modulesPath})
    } catch (err) {
      if (!opts.force) throw err
      if (installType !== 'general') {
        throw new Error('Named installation cannot be used to regenerate the node_modules structure. Run pnpm install --force')
      }
      logger.info({
        message: `Recreating ${modulesPath}`,
        prefix: opts.prefix,
      })
      await removeAllExceptOuterLinks(modulesPath)
      return getContext(opts)
    }
  }

  const files = await Promise.all([
    (opts.global ? readGlobalPkgJson(opts.prefix) : safeReadPkgFromDir(opts.prefix)),
    mkdirp(storePath),
  ])
  const pkg = files[0] || {} as PackageJson
  const ctx: PnpmContext = {
    hoistedAliases: modules && modules.hoistedAliases || {},
    pendingBuilds: modules && modules.pendingBuilds || [],
    pkg: opts.hooks && opts.hooks.readPackage ? opts.hooks.readPackage(pkg) : pkg,
    prefix: opts.prefix,
    skipped: new Set(modules && modules.skipped || []),
    storePath,
    ...await readShrinkwrapFile(opts),
  }
  packageJsonLogger.debug({
    initial: ctx.pkg,
    prefix: opts.prefix,
  })

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
