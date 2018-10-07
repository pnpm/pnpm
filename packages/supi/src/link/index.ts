import {
  packageJsonLogger,
  summaryLogger,
} from '@pnpm/core-loggers'
import { linkBinsOfPackages } from '@pnpm/link-bins'
import logger, { streamParser } from '@pnpm/logger'
import { prune } from '@pnpm/modules-cleaner'
import {
  DEPENDENCIES_FIELDS,
  DependenciesField,
  PackageJson,
} from '@pnpm/types'
import {
  getSaveType,
  safeReadPackage,
} from '@pnpm/utils'
import loadJsonFile from 'load-json-file'
import normalize = require('normalize-path')
import path = require('path')
import pathAbsolute = require('path-absolute')
import {
  getImporterId,
  pruneSharedShrinkwrap,
  ShrinkwrapImporter,
  write as saveShrinkwrap,
  writeCurrentOnly as saveCurrentShrinkwrapOnly,
} from 'pnpm-shrinkwrap'
import R = require('ramda')
import { getContextForSingleImporter } from '../getContext'
import getSpecFromPackageJson from '../getSpecFromPackageJson'
import linkToModules from '../linkToModules'
import save, { guessDependencyType } from '../save'
import getPref from '../utils/getPref'
import {
  extendOptions,
  LinkOptions,
} from './options'

export default async function link (
  linkFromPkgs: Array<{alias: string, path: string} | string>,
  destModules: string,
  maybeOpts: LinkOptions & {
    linkToBin?: string,
  },
) {
  const reporter = maybeOpts && maybeOpts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }
  maybeOpts.saveProd = maybeOpts.saveProd === true
  const opts = await extendOptions(maybeOpts)
  const ctx = await getContextForSingleImporter(opts)

  const importerId = getImporterId(ctx.shrinkwrapDirectory, opts.prefix)
  const oldShrinkwrap = R.clone(ctx.currentShrinkwrap)
  const pkg = await safeReadPackage(path.join(opts.prefix, 'package.json')) || undefined
  if (pkg) {
    packageJsonLogger.debug({
      initial: pkg,
      prefix: opts.prefix,
    })
  }
  const linkedPkgs: Array<{path: string, pkg: PackageJson, alias: string}> = []
  const specsToUpsert = [] as Array<{name: string, pref: string, saveType: DependenciesField}>
  const saveType = getSaveType(opts)

  for (const linkFrom of linkFromPkgs) {
    let linkFromPath: string
    let linkFromAlias: string | undefined
    if (typeof linkFrom === 'string') {
      linkFromPath = linkFrom
    } else {
      linkFromPath = linkFrom.path
      linkFromAlias = linkFrom.alias
    }
    const linkedPkg = await loadJsonFile<PackageJson>(path.join(linkFromPath, 'package.json'))
    specsToUpsert.push({
      name: linkedPkg.name,
      pref: getPref(linkedPkg.name, linkedPkg.name, linkedPkg.version, {
        saveExact: opts.saveExact,
        savePrefix: opts.savePrefix,
      }),
      saveType: (saveType || pkg && guessDependencyType(linkedPkg.name, pkg)) as DependenciesField,
    })

    const packagePath = normalize(path.relative(ctx.shrinkwrapDirectory, linkFromPath))
    const addLinkOpts = {
      linkedPkgName: linkFromAlias || linkedPkg.name,
      packagePath,
      pkg,
    }
    addLinkToShrinkwrap(ctx.currentShrinkwrap.importers[importerId], addLinkOpts)
    addLinkToShrinkwrap(ctx.wantedShrinkwrap.importers[importerId], addLinkOpts)

    linkedPkgs.push({
      alias: linkFromAlias || linkedPkg.name,
      path: linkFromPath,
      pkg: linkedPkg,
    })
  }

  const updatedCurrentShrinkwrap = pruneSharedShrinkwrap(ctx.currentShrinkwrap)

  const warn = (message: string) => logger.warn({ message, prefix: opts.prefix })
  const updatedWantedShrinkwrap = pruneSharedShrinkwrap(ctx.wantedShrinkwrap, warn)

  await prune({
    importers: [
      {
        bin: opts.bin,
        hoistedAliases: ctx.hoistedAliases,
        id: importerId,
        modulesDir: ctx.modulesDir,
        prefix: opts.prefix,
        shamefullyFlatten: opts.shamefullyFlatten,
      },
    ],
    newShrinkwrap: updatedCurrentShrinkwrap,
    oldShrinkwrap,
    storeController: opts.storeController,
    virtualStoreDir: ctx.virtualStoreDir,
  })

  // Linking should happen after removing orphans
  // Otherwise would've been removed
  for (const linkedPkg of linkedPkgs) {
    // TODO: cover with test that linking reports with correct dependency types
    const stu = specsToUpsert.find((s) => s.name === linkedPkg.pkg.name)
    await linkToModules({
      alias: linkedPkg.alias,
      destModulesDir: destModules,
      name: linkedPkg.pkg.name,
      packageDir: linkedPkg.path,
      prefix: opts.prefix,
      saveType: stu && stu.saveType || saveType,
      version: linkedPkg.pkg.version,
    })
  }

  const linkToBin = maybeOpts && maybeOpts.linkToBin || path.join(destModules, '.bin')
  await linkBinsOfPackages(linkedPkgs.map((p) => ({ manifest: p.pkg, location: p.path })), linkToBin, {
    warn: (message: string) => logger.warn({ message, prefix: opts.prefix }),
  })

  if (opts.saveDev || opts.saveProd || opts.saveOptional) {
    const newPkg = await save(opts.prefix, specsToUpsert)
    for (const specToUpsert of specsToUpsert) {
      updatedWantedShrinkwrap.importers[importerId].specifiers[specToUpsert.name] = getSpecFromPackageJson(newPkg, specToUpsert.name)
    }
  }
  if (opts.shrinkwrap) {
    await saveShrinkwrap(ctx.shrinkwrapDirectory, updatedWantedShrinkwrap, updatedCurrentShrinkwrap)
  } else {
    await saveCurrentShrinkwrapOnly(ctx.shrinkwrapDirectory, updatedCurrentShrinkwrap)
  }

  summaryLogger.debug({ prefix: opts.prefix })

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }
}

function addLinkToShrinkwrap (
  shrImporter: ShrinkwrapImporter,
  opts: {
    linkedPkgName: string,
    packagePath: string,
    pkg?: PackageJson,
  },
) {
  const id = `link:${opts.packagePath}`
  let addedTo: DependenciesField | undefined
  for (const depType of DEPENDENCIES_FIELDS) {
    if (!addedTo && opts.pkg && opts.pkg[depType] && opts.pkg[depType]![opts.linkedPkgName]) {
      addedTo = depType
      shrImporter[depType] = shrImporter[depType] || {}
      shrImporter[depType]![opts.linkedPkgName] = id
    } else if (shrImporter[depType]) {
      delete shrImporter[depType]![opts.linkedPkgName]
    }
  }

  if (!addedTo) {
    shrImporter.dependencies = shrImporter.dependencies || {}
    shrImporter.dependencies[opts.linkedPkgName] = id
  }

  // package.json might not be available when linking to global
  if (!opts.pkg) return

  const availableSpec = getSpecFromPackageJson(opts.pkg, opts.linkedPkgName)
  if (availableSpec) {
    shrImporter.specifiers[opts.linkedPkgName] = availableSpec
  } else {
    delete shrImporter.specifiers[opts.linkedPkgName]
  }
}

export async function linkFromGlobal (
  pkgNames: string[],
  linkTo: string,
  maybeOpts: LinkOptions & {globalPrefix: string},
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
  maybeOpts: LinkOptions & {
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
