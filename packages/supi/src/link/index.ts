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
import readImporterManifest from '@pnpm/read-importer-manifest'
import { symlinkDirectRootDependency } from '@pnpm/symlink-dependency'
import {
  DEPENDENCIES_FIELDS,
  DependenciesField,
  DependencyManifest,
  ImporterManifest,
} from '@pnpm/types'
import normalize = require('normalize-path')
import path = require('path')
import pathAbsolute = require('path-absolute')
import R = require('ramda')
import { getContextForSingleImporter } from '../getContext'
import getSpecFromPackageManifest from '../getSpecFromPackageManifest'
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
  const opts = await extendOptions(maybeOpts)
  const ctx = await getContextForSingleImporter(opts.manifest, {
    ...opts,
    extraBinPaths: [], // ctx.extraBinPaths is not needed, so this is fine
  })

  const importerId = getLockfileImporterId(ctx.lockfileDirectory, opts.prefix)
  const currentLockfile = R.clone(ctx.currentLockfile)
  const linkedPkgs: Array<{path: string, manifest: DependencyManifest, alias: string}> = []
  const specsToUpsert = [] as Array<{name: string, pref: string, saveType: DependenciesField}>

  for (const linkFrom of linkFromPkgs) {
    let linkFromPath: string
    let linkFromAlias: string | undefined
    if (typeof linkFrom === 'string') {
      linkFromPath = linkFrom
    } else {
      linkFromPath = linkFrom.path
      linkFromAlias = linkFrom.alias
    }
    const { manifest } = await readImporterManifest(linkFromPath) as { manifest: DependencyManifest }
    specsToUpsert.push({
      name: manifest.name,
      pref: getPref(manifest.name, manifest.name, manifest.version, {
        pinnedVersion: opts.pinnedVersion,
      }),
      saveType: (opts.targetDependenciesField || ctx.manifest && guessDependencyType(manifest.name, ctx.manifest)) as DependenciesField,
    })

    const packagePath = normalize(path.relative(opts.prefix, linkFromPath))
    const addLinkOpts = {
      linkedPkgName: linkFromAlias || manifest.name,
      manifest: ctx.manifest,
      packagePath,
    }
    addLinkToLockfile(ctx.currentLockfile.importers[importerId], addLinkOpts)
    addLinkToLockfile(ctx.wantedLockfile.importers[importerId], addLinkOpts)

    linkedPkgs.push({
      alias: linkFromAlias || manifest.name,
      manifest,
      path: linkFromPath,
    })
  }

  const updatedCurrentLockfile = pruneSharedLockfile(ctx.currentLockfile)

  const warn = (message: string) => logger.warn({ message, prefix: opts.prefix })
  const updatedWantedLockfile = pruneSharedLockfile(ctx.wantedLockfile, { warn })

  await prune(
    [
      {
        bin: opts.bin,
        id: importerId,
        modulesDir: ctx.modulesDir,
        prefix: opts.prefix,
      },
    ],
    {
      currentLockfile,
      hoistedAliases: ctx.hoistedAliases,
      hoistedModulesDir: opts.hoistPattern && ctx.hoistedModulesDir || undefined,
      include: ctx.include,
      lockfileDirectory: opts.lockfileDirectory,
      registries: ctx.registries,
      skipped: ctx.skipped,
      storeController: opts.storeController,
      virtualStoreDir: ctx.virtualStoreDir,
      wantedLockfile: updatedCurrentLockfile,
    },
  )

  // Linking should happen after removing orphans
  // Otherwise would've been removed
  for (const { alias, manifest, path } of linkedPkgs) {
    // TODO: cover with test that linking reports with correct dependency types
    const stu = specsToUpsert.find((s) => s.name === manifest.name)
    await symlinkDirectRootDependency(path, destModules, alias, {
      fromDependenciesField: stu?.saveType ?? opts.targetDependenciesField,
      linkedPackage: manifest,
      prefix: opts.prefix,
    })
  }

  const linkToBin = maybeOpts?.linkToBin || path.join(destModules, '.bin')
  await linkBinsOfPackages(linkedPkgs.map((p) => ({ manifest: p.manifest, location: p.path })), linkToBin, {
    warn: (message: string) => logger.warn({ message, prefix: opts.prefix }),
  })

  let newPkg!: ImporterManifest
  if (opts.targetDependenciesField) {
    newPkg = await save(opts.prefix, opts.manifest, specsToUpsert)
    for (const { name } of specsToUpsert) {
      updatedWantedLockfile.importers[importerId].specifiers[name] = getSpecFromPackageManifest(newPkg, name)
    }
  } else {
    newPkg = opts.manifest
  }
  const lockfileOpts = { forceSharedFormat: opts.forceSharedLockfile }
  if (opts.useLockfile) {
    await writeLockfiles({
      currentLockfile: updatedCurrentLockfile,
      currentLockfileDir: ctx.virtualStoreDir,
      wantedLockfile: updatedWantedLockfile,
      wantedLockfileDir: ctx.lockfileDirectory,
      ...lockfileOpts,
    })
  } else {
    await writeCurrentLockfile(ctx.virtualStoreDir, updatedCurrentLockfile, lockfileOpts)
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
    manifest?: ImporterManifest,
  },
) {
  const id = `link:${opts.packagePath}`
  let addedTo: DependenciesField | undefined
  for (const depType of DEPENDENCIES_FIELDS) {
    if (!addedTo && opts.manifest?.[depType]?.[opts.linkedPkgName]) {
      addedTo = depType
      lockfileImporter[depType] = lockfileImporter[depType] || {}
      lockfileImporter[depType]![opts.linkedPkgName] = id
    } else if (lockfileImporter[depType]) {
      delete lockfileImporter[depType]![opts.linkedPkgName]
    }
  }

  // package.json might not be available when linking to global
  if (!opts.manifest) return

  const availableSpec = getSpecFromPackageManifest(opts.manifest, opts.linkedPkgName)
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
  const reporter = maybeOpts?.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }
  const opts = await extendOptions(maybeOpts)
  const globalPkgPath = pathAbsolute(maybeOpts.globalPrefix)
  const linkFromPkgs = pkgNames.map((pkgName) => path.join(globalPkgPath, 'node_modules', pkgName))
  const newManifest = await link(linkFromPkgs, path.join(linkTo, 'node_modules'), opts)

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }

  return newManifest
}

export async function linkToGlobal (
  linkFrom: string,
  maybeOpts: LinkOptions & {
    globalBin: string,
    globalPrefix: string,
  },
) {
  const reporter = maybeOpts?.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }
  maybeOpts.lockfileDirectory = maybeOpts.lockfileDirectory || maybeOpts.globalPrefix
  const opts = await extendOptions(maybeOpts)
  const globalPkgPath = pathAbsolute(maybeOpts.globalPrefix)
  const newManifest = await link([linkFrom], path.join(globalPkgPath, 'node_modules'), {
    ...opts,
    linkToBin: maybeOpts.globalBin,
    prefix: maybeOpts.globalPrefix,
  })

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }

  return newManifest
}
