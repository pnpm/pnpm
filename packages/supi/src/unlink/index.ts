import logger, { streamParser } from '@pnpm/logger'
import { fromDir as readPkgFromDir } from '@pnpm/read-package-json'
import { getAllDependenciesFromPackage } from '@pnpm/utils'
import isInnerLink = require('is-inner-link')
import isSubdir = require('is-subdir')
import fs = require('mz/fs')
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
  opts.importers = ctx.importers

  await _unlinkPkgs(pkgNames, opts)

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }
}

export async function _unlinkPkgs (
  pkgNames: string[],
  opts: StrictInstallOptions,
) {
  if (opts.importers.length > 1) throw new Error('Unlink not implemented for multiple importers yet')
  const importer = opts.importers[0]
  const pkg = await readPkgFromDir(importer.prefix)
  const allDeps = getAllDependenciesFromPackage(pkg)
  const packagesToInstall: string[] = []

  for (const pkgName of pkgNames) {
    try {
      if (!await isExternalLink(opts.store, importer.importerModulesDir, pkgName)) {
        logger.warn({
          message: `${pkgName} is not an external link`,
          prefix: importer.prefix,
        })
        continue
      }
    } catch (err) {
      if (err['code'] !== 'ENOENT') throw err // tslint:disable-line:no-string-literal
    }
    await rimraf(path.join(importer.importerModulesDir, pkgName))
    if (allDeps[pkgName]) {
      packagesToInstall.push(pkgName)
    }
  }

  if (!packagesToInstall.length) return

  // TODO: install only those that were unlinked
  // but don't update their version specs in package.json
  await install({...opts, preferFrozenShrinkwrap: false})
}

export async function unlink (maybeOpts: InstallOptions) {
  const reporter = maybeOpts && maybeOpts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }
  const opts = await _extendOptions(maybeOpts)
  const ctx = await getContext(opts)
  opts.store = ctx.storePath
  opts.importers = ctx.importers

  if (opts.importers.length > 1) throw new Error('Unlink not implemented for multiple importers yet')
  const importer = opts.importers[0]

  const externalPackages = await getExternalPackages(importer.importerModulesDir, opts.store)

  await _unlinkPkgs(externalPackages, opts)

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }
}

async function getExternalPackages (
  modules: string,
  store: string,
  scope?: string,
): Promise<string[]> {
  let externalLinks: string[] = []
  const parentDir = scope ? path.join(modules, scope) : modules
  for (const dir of await fs.readdir(parentDir)) {
    if (dir[0] === '.') continue

    if (!scope && dir[0] === '@') {
      externalLinks = externalLinks.concat(await getExternalPackages(modules, store, dir))
      continue
    }

    const pkgName = scope ? `${scope}/${dir}` : dir

    if (await isExternalLink(store, modules, pkgName)) {
      externalLinks.push(pkgName)
    }
  }
  return externalLinks
}

async function isExternalLink (store: string, modules: string, pkgName: string) {
  const link = await isInnerLink(modules, pkgName)

  // checking whether the link is pointing to the store is needed
  // because packages are linked to store when independent-leaves = true
  return !link.isInner && !isSubdir(store, link.target)
}

async function _extendOptions (maybeOpts: InstallOptions): Promise<StrictInstallOptions> {
  maybeOpts = maybeOpts || {}
  if (maybeOpts.depth === undefined) maybeOpts.depth = -1
  return await extendOptions(maybeOpts)
}
