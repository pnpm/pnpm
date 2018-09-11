import { packageJsonLogger } from '@pnpm/core-loggers'
import logger from '@pnpm/logger'
import { read as readModulesYaml } from '@pnpm/modules-yaml'
import {
  PackageJson,
  ReadPackageHook,
} from '@pnpm/types'
import {
  safeReadPackageFromDir as safeReadPkgFromDir,
} from '@pnpm/utils'
import mkdirp = require('mkdirp-promise')
import path = require('path')
import { Shrinkwrap } from 'pnpm-shrinkwrap'
import removeAllExceptOuterLinks = require('remove-all-except-outer-links')
import { PnpmError } from '../errorTypes'
import readShrinkwrapFile from '../readShrinkwrapFiles'
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
            'This "node_modules" folder was created using the --independent-leaves option.'
            + ' You must add that option, or else add the --force option to recreate the "node_modules" folder.',
          )
        }
        throw new PnpmError(
          'ERR_PNPM_INDEPENDENT_LEAVES_NOT_WANTED',
          'This "node_modules" folder was created without the --independent-leaves option.'
          + ' You must remove that option, or else add the --force option to recreate the "node_modules" folder.',
        )
      }
      if (Boolean(modules.shamefullyFlatten) !== opts.shamefullyFlatten) {
        if (modules.shamefullyFlatten) {
          throw new PnpmError(
            'ERR_PNPM_SHAMEFULLY_FLATTEN_WANTED',
            'This "node_modules" folder was created using the --shamefully-flatten option.'
            + ' You must add this option, or else add the --force option to recreate the "node_modules" folder.',
          )
        }
        throw new PnpmError(
          'ERR_PNPM_SHAMEFULLY_FLATTEN_NOT_WANTED',
          'This "node_modules" folder was created without the --shamefully-flatten option.'
          + ' You must remove that option, or else add the --force option to recreate the "node_modules" folder.',
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
    safeReadPkgFromDir(opts.prefix),
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
