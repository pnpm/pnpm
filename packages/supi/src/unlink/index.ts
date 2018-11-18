import logger, { streamParser } from '@pnpm/logger'
import readModulesDirs from '@pnpm/read-modules-dir'
import { fromDir as readPkgFromDir } from '@pnpm/read-package-json'
import { getAllDependenciesFromPackage } from '@pnpm/utils'
import isInnerLink = require('is-inner-link')
import isSubdir = require('is-subdir')
import pFilter = require('p-filter')
import path = require('path')
import rimraf = require('rimraf-then')
import getContext from '../getContext'
import { install } from '../install'
import extendOptions, {
  InstallOptions,
  StrictInstallOptions,
} from '../install/extendInstallOptions'

export async function unlinkPkgs (
  pkgNames: string[],
  maybeOpts: InstallOptions,
) {
  const reporter = maybeOpts && maybeOpts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }
  const opts = await _extendOptions(maybeOpts)
  const ctx = await getContext(opts)
  opts.store = ctx.storePath

  await _unlinkPkgs(pkgNames, opts, ctx.importers)

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }
}

export async function _unlinkPkgs (
  pkgNames: string[],
  opts: StrictInstallOptions,
  importers: Array<{ modulesDir: string, prefix: string }>,
) {
  if (importers.length > 1) throw new Error('Unlink not implemented for multiple importers yet')
  const importer = importers[0]
  const pkg = await readPkgFromDir(importer.prefix)
  const allDeps = getAllDependenciesFromPackage(pkg)
  const packagesToInstall: string[] = []

  for (const pkgName of pkgNames) {
    try {
      if (!await isExternalLink(opts.store, importer.modulesDir, pkgName)) {
        logger.warn({
          message: `${pkgName} is not an external link`,
          prefix: importer.prefix,
        })
        continue
      }
    } catch (err) {
      if (err['code'] !== 'ENOENT') throw err // tslint:disable-line:no-string-literal
    }
    await rimraf(path.join(importer.modulesDir, pkgName))
    if (allDeps[pkgName]) {
      packagesToInstall.push(pkgName)
    }
  }

  if (!packagesToInstall.length) return

  // TODO: install only those that were unlinked
  // but don't update their version specs in package.json
  await install({ ...opts, preferFrozenShrinkwrap: false })
}

export async function unlink (maybeOpts: InstallOptions) {
  const reporter = maybeOpts && maybeOpts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }
  const opts = await _extendOptions(maybeOpts)
  const ctx = await getContext(opts)
  opts.store = ctx.storePath

  if (ctx.importers.length > 1) throw new Error('Unlink not implemented for multiple importers yet')
  const importer = ctx.importers[0]

  const packageDirs = await readModulesDirs(importer.modulesDir)
  const externalPackages = await pFilter(
    packageDirs,
    (packageDir: string) => isExternalLink(opts.store, importer.modulesDir, packageDir),
  )

  await _unlinkPkgs(externalPackages, opts, ctx.importers)

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }
}

async function isExternalLink (store: string, modules: string, pkgName: string) {
  const link = await isInnerLink(modules, pkgName)

  // checking whether the link is pointing to the store is needed
  // because packages are linked to store when independent-leaves = true
  return !link.isInner && !isSubdir(store, link.target)
}

function _extendOptions (maybeOpts: InstallOptions): Promise<StrictInstallOptions> {
  maybeOpts = maybeOpts || {}
  if (maybeOpts.depth === undefined) maybeOpts.depth = -1
  return extendOptions(maybeOpts)
}
