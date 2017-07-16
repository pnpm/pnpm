import path = require('path')
import {PnpmOptions, StrictPnpmOptions} from '../types'
import streamParser from '../logging/streamParser'
import extendOptions from './extendOptions'
import isInnerLink = require('is-inner-link')
import logger from 'pnpm-logger'
import rimraf = require('rimraf-then')
import {installPkgs} from './install'
import {fromDir as readPkgFromDir} from '../fs/readPkg'
import depsFromPackage from '../depsFromPackage'
import fs = require('mz/fs')
import {
  read as readModules,
} from '../fs/modulesController'
import isSubdir = require('is-subdir')

export async function unlinkPkgs (
  pkgNames: string[],
  maybeOpts: PnpmOptions
) {
  const reporter = maybeOpts && maybeOpts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }
  const opts = extendOptions(maybeOpts)
  const modulesYaml = await readModules(opts.prefix)
  opts.store = modulesYaml && modulesYaml.store || opts.store

  await _unlinkPkgs(pkgNames, opts)

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }
}

export async function _unlinkPkgs (
  pkgNames: string[],
  opts: StrictPnpmOptions
) {
  const modules = path.join(opts.prefix, 'node_modules')
  const pkg = await readPkgFromDir(opts.prefix)
  const allDeps = depsFromPackage(pkg)
  const packagesToInstall: string[] = []

  for (const pkgName of pkgNames) {
    if (!await isExternalLink(opts.store, modules, pkgName)) {
      logger.warn(`${pkgName} is not an external link`)
      continue
    }
    await rimraf(path.join(modules, pkgName))
    if (allDeps[pkgName]) {
      packagesToInstall.push(pkgName)
    }
  }

  if (!packagesToInstall.length) return

  await installPkgs(packagesToInstall, opts)
}

export async function unlink (maybeOpts: PnpmOptions) {
  const reporter = maybeOpts && maybeOpts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }
  const opts = extendOptions(maybeOpts)
  const modulesYaml = await readModules(opts.prefix)
  opts.store = modulesYaml && modulesYaml.store || opts.store

  const modules = path.join(opts.prefix, 'node_modules')

  const externalPackages = await getExternalPackages(modules, opts.store)

  await _unlinkPkgs(externalPackages, opts)

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }
}

async function getExternalPackages (
  modules: string,
  store: string,
  scope?: string
): Promise<string[]> {
  let externalLinks: string[] = []
  for (const dir of await fs.readdir(modules)) {
    if (dir[0] === '.') continue

    if (!scope && dir[0] === '@') {
      externalLinks = externalLinks.concat(await getExternalPackages(path.join(modules, dir), store, dir))
      continue
    }

    if (await isExternalLink(store, modules, dir)) {
      externalLinks.push(scope ? `${scope}/${dir}` : dir)
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
