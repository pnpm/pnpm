import {linkBinsOfPackages} from '@pnpm/link-bins'
import logger, {streamParser} from '@pnpm/logger'
import {read as readModulesYaml} from '@pnpm/modules-yaml'
import {PackageJson} from '@pnpm/types'
import {
  DependenciesType,
  dependenciesTypes,
  removeOrphanPackages as removeOrphanPkgs,
  safeReadPackage,
} from '@pnpm/utils'
import loadJsonFile = require('load-json-file')
import normalize = require('normalize-path')
import pLimit = require('p-limit')
import path = require('path')
import pathAbsolute = require('path-absolute')
import {
  prune as pruneShrinkwrap,
  Shrinkwrap,
  write as saveShrinkwrap,
  writeCurrentOnly as saveCurrentShrinkwrapOnly,
} from 'pnpm-shrinkwrap'
import R = require('ramda')
import symlinkDir = require('symlink-dir')
import getSpecFromPackageJson from '../getSpecFromPackageJson'
import readShrinkwrapFile from '../readShrinkwrapFiles'
import extendOptions, {
  InstallOptions,
} from './extendInstallOptions'
import {install} from './install'

const linkLogger = logger('link')
const installLimit = pLimit(4)

export default async function link (
  linkFromPkgs: string[],
  destModules: string,
  maybeOpts: InstallOptions & {
    skipInstall?: boolean,
    linkToBin?: string,
  },
) {
  const reporter = maybeOpts && maybeOpts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }
  const opts = await extendOptions(maybeOpts)

  if (!maybeOpts || !maybeOpts.skipInstall) {
    await Promise.all(
      linkFromPkgs.map((prefix) => installLimit(() =>
        install({
          ...opts,
          bin: path.join(prefix, 'node_modules', '.bin'),
          global: false,
          prefix,
        }),
      )),
    )
  }
  const shrFiles = await readShrinkwrapFile({
    force: opts.force,
    prefix: opts.prefix,
    registry: opts.registry,
    shrinkwrap: opts.shrinkwrap,
  })
  const oldShrinkwrap = R.clone(shrFiles.currentShrinkwrap)
  const pkg = await safeReadPackage(path.join(opts.prefix, 'package.json')) || undefined
  const linkedPkgs: Array<{path: string, pkg: PackageJson}> = []

  for (const linkFrom of linkFromPkgs) {
    const linkedPkg = await loadJsonFile(path.join(linkFrom, 'package.json'))

    const packagePath = normalize(path.relative(opts.prefix, linkFrom))
    const addLinkOpts = {
      linkedPkgName: linkedPkg.name,
      packagePath,
      pkg,
    }
    addLinkToShrinkwrap(shrFiles.currentShrinkwrap, addLinkOpts)
    addLinkToShrinkwrap(shrFiles.wantedShrinkwrap, addLinkOpts)

    linkedPkgs.push({path: linkFrom, pkg: linkedPkg})
  }

  const updatedCurrentShrinkwrap = pruneShrinkwrap(shrFiles.currentShrinkwrap)
  const updatedWantedShrinkwrap = pruneShrinkwrap(shrFiles.wantedShrinkwrap)
  const modulesInfo = await readModulesYaml(destModules)
  await removeOrphanPkgs({
    bin: opts.bin,
    hoistedAliases: modulesInfo && modulesInfo.hoistedAliases || {},
    newShrinkwrap: updatedCurrentShrinkwrap,
    oldShrinkwrap,
    prefix: opts.prefix,
    shamefullyFlatten: opts.shamefullyFlatten,
    storeController: opts.storeController,
  })

  // Linking should happen after removing orphans
  // Otherwise would've been removed
  for (const linkedPkg of linkedPkgs) {
    await linkToModules(linkedPkg.pkg.name, linkedPkg.path, destModules)
  }

  const linkToBin = maybeOpts && maybeOpts.linkToBin || path.join(destModules, '.bin')
  await linkBinsOfPackages(linkedPkgs.map((p) => ({manifest: p.pkg, location: p.path})), linkToBin)

  if (opts.shrinkwrap) {
    await saveShrinkwrap(opts.prefix, updatedWantedShrinkwrap, updatedCurrentShrinkwrap)
  } else {
    await saveCurrentShrinkwrapOnly(opts.prefix, updatedCurrentShrinkwrap)
  }

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }
}

function addLinkToShrinkwrap (
  shr: Shrinkwrap,
  opts: {
    linkedPkgName: string,
    packagePath: string,
    pkg?: PackageJson,
  },
) {
  const id = `link:${opts.packagePath}`
  let addedTo: DependenciesType | undefined
  for (const depType of dependenciesTypes) {
    if (opts.pkg && opts.pkg[depType]) {
      addedTo = depType
      shr[depType] = shr[depType] || {}
      shr[depType]![opts.linkedPkgName] = id
    } else if (shr[depType]) {
      delete shr[depType]![opts.linkedPkgName]
    }
  }

  if (!addedTo) {
    shr.dependencies = shr.dependencies || {}
    shr.dependencies[opts.linkedPkgName] = id
  }

  // package.json might not be available when linking to global
  if (!opts.pkg) return

  const availableSpec = getSpecFromPackageJson(opts.pkg, opts.linkedPkgName)
  if (availableSpec) {
    shr.specifiers[opts.linkedPkgName] = availableSpec
  } else {
    delete shr.specifiers[opts.linkedPkgName]
  }
}

async function linkToModules (pkgName: string, linkFrom: string, modules: string) {
  const dest = path.join(modules, pkgName)
  linkLogger.info(`${dest} -> ${linkFrom}`)
  await symlinkDir(linkFrom, dest)
}

export async function linkFromGlobal (
  pkgNames: string[],
  linkTo: string,
  maybeOpts: InstallOptions & {globalPrefix: string},
) {
  const reporter = maybeOpts && maybeOpts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }
  const opts = await extendOptions(maybeOpts)
  const globalPkgPath = pathAbsolute(maybeOpts.globalPrefix)
  const linkFromPkgs = pkgNames.map((pkgName) => path.join(globalPkgPath, 'node_modules', pkgName))
  await link(linkFromPkgs, path.join(linkTo, 'node_modules'), opts)

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }
}

export async function linkToGlobal (
  linkFrom: string,
  maybeOpts: InstallOptions & {
    globalBin: string,
    globalPrefix: string,
  },
) {
  const reporter = maybeOpts && maybeOpts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }
  const opts = await extendOptions(maybeOpts)
  const globalPkgPath = pathAbsolute(maybeOpts.globalPrefix)
  await link([linkFrom], path.join(globalPkgPath, 'node_modules'), {
    ...opts,
    linkToBin: maybeOpts.globalBin,
    prefix: maybeOpts.globalPrefix,
  })

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }
}
