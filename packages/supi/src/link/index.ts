import {
  summaryLogger,
} from '@pnpm/core-loggers'
import { linkBinsOfPackages } from '@pnpm/link-bins'
import {
  getLockfileImporterId,
  LockfileImporter,
  writeCurrentLockfile,
  writeLockfiles,
} from '@pnpm/lockfile-file'
import logger, { streamParser } from '@pnpm/logger'
import { prune } from '@pnpm/modules-cleaner'
import { pruneSharedLockfile } from '@pnpm/prune-lockfile'
import { symlinkDirectRootDependency } from '@pnpm/symlink-dependency'
import {
  DEPENDENCIES_FIELDS,
  DependenciesField,
  DependencyPackageJson,
  PackageJson,
} from '@pnpm/types'
import {
  getSaveType,
} from '@pnpm/utils'
import loadJsonFile from 'load-json-file'
import normalize = require('normalize-path')
import path = require('path')
import pathAbsolute = require('path-absolute')
import R = require('ramda')
import { getContextForSingleImporter } from '../getContext'
import getSpecFromPackageJson from '../getSpecFromPackageJson'
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
    prefix: string,
  },
) {
  const reporter = maybeOpts && maybeOpts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }
  maybeOpts.saveProd = maybeOpts.saveProd === true
  const opts = await extendOptions(maybeOpts)
  const ctx = await getContextForSingleImporter(opts.pkg, opts)

  const importerId = getLockfileImporterId(ctx.lockfileDirectory, opts.prefix)
  const oldLockfile = R.clone(ctx.currentLockfile)
  const linkedPkgs: Array<{path: string, pkg: DependencyPackageJson, alias: string}> = []
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
    const linkedPkg = await loadJsonFile<DependencyPackageJson>(path.join(linkFromPath, 'package.json'))
    specsToUpsert.push({
      name: linkedPkg.name,
      pref: getPref(linkedPkg.name, linkedPkg.name, linkedPkg.version, {
        pinnedVersion: opts.pinnedVersion,
      }),
      saveType: (saveType || ctx.pkg && guessDependencyType(linkedPkg.name, ctx.pkg)) as DependenciesField,
    })

    const packagePath = normalize(path.relative(opts.prefix, linkFromPath))
    const addLinkOpts = {
      linkedPkgName: linkFromAlias || linkedPkg.name,
      packagePath,
      pkg: ctx.pkg,
    }
    addLinkToLockfile(ctx.currentLockfile.importers[importerId], addLinkOpts)
    addLinkToLockfile(ctx.wantedLockfile.importers[importerId], addLinkOpts)

    linkedPkgs.push({
      alias: linkFromAlias || linkedPkg.name,
      path: linkFromPath,
      pkg: linkedPkg,
    })
  }

  const updatedCurrentLockfile = pruneSharedLockfile(ctx.currentLockfile, { defaultRegistry: opts.registries.default })

  const warn = (message: string) => logger.warn({ message, prefix: opts.prefix })
  const updatedWantedLockfile = pruneSharedLockfile(ctx.wantedLockfile, {
    defaultRegistry: opts.registries.default,
    warn,
  })

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
    lockfileDirectory: opts.lockfileDirectory,
    newLockfile: updatedCurrentLockfile,
    oldLockfile,
    registries: ctx.registries,
    storeController: opts.storeController,
    virtualStoreDir: ctx.virtualStoreDir,
  })

  // Linking should happen after removing orphans
  // Otherwise would've been removed
  for (const linkedPkg of linkedPkgs) {
    // TODO: cover with test that linking reports with correct dependency types
    const stu = specsToUpsert.find((s) => s.name === linkedPkg.pkg.name)
    await symlinkDirectRootDependency(linkedPkg.path, destModules, linkedPkg.alias, {
      fromDependenciesField: stu && stu.saveType || saveType,
      linkedPackage: linkedPkg.pkg,
      prefix: opts.prefix,
    })
  }

  const linkToBin = maybeOpts && maybeOpts.linkToBin || path.join(destModules, '.bin')
  await linkBinsOfPackages(linkedPkgs.map((p) => ({ manifest: p.pkg, location: p.path })), linkToBin, {
    warn: (message: string) => logger.warn({ message, prefix: opts.prefix }),
  })

  let newPkg!: PackageJson
  if (opts.saveDev || opts.saveProd || opts.saveOptional) {
    newPkg = await save(opts.prefix, opts.pkg, specsToUpsert)
    for (const specToUpsert of specsToUpsert) {
      updatedWantedLockfile.importers[importerId].specifiers[specToUpsert.name] = getSpecFromPackageJson(newPkg, specToUpsert.name)
    }
  } else {
    newPkg = opts.pkg
  }
  const lockfileOpts = { forceSharedFormat: opts.forceSharedLockfile }
  if (opts.useLockfile) {
    await writeLockfiles(ctx.lockfileDirectory, updatedWantedLockfile, updatedCurrentLockfile, lockfileOpts)
  } else {
    await writeCurrentLockfile(ctx.lockfileDirectory, updatedCurrentLockfile, lockfileOpts)
  }

  summaryLogger.debug({ prefix: opts.prefix })

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }

  return newPkg
}

function addLinkToLockfile (
  lockfileImporter: LockfileImporter,
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
      lockfileImporter[depType] = lockfileImporter[depType] || {}
      lockfileImporter[depType]![opts.linkedPkgName] = id
    } else if (lockfileImporter[depType]) {
      delete lockfileImporter[depType]![opts.linkedPkgName]
    }
  }

  // package.json might not be available when linking to global
  if (!opts.pkg) return

  const availableSpec = getSpecFromPackageJson(opts.pkg, opts.linkedPkgName)
  if (availableSpec) {
    lockfileImporter.specifiers[opts.linkedPkgName] = availableSpec
  } else {
    delete lockfileImporter.specifiers[opts.linkedPkgName]
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
  maybeOpts.lockfileDirectory = maybeOpts.lockfileDirectory || maybeOpts.globalPrefix
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
