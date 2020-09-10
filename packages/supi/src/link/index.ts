import {
  summaryLogger,
} from '@pnpm/core-loggers'
import PnpmError from '@pnpm/error'
import { getContextForSingleImporter } from '@pnpm/get-context'
import { linkBinsOfPackages } from '@pnpm/link-bins'
import {
  getLockfileImporterId,
  ProjectSnapshot,
  writeCurrentLockfile,
  writeLockfiles,
} from '@pnpm/lockfile-file'
import logger, { streamParser } from '@pnpm/logger'
import {
  getPref,
  getSpecFromPackageManifest,
  guessDependencyType,
  PackageSpecObject,
  updateProjectManifestObject,
} from '@pnpm/manifest-utils'
import { prune } from '@pnpm/modules-cleaner'
import { pruneSharedLockfile } from '@pnpm/prune-lockfile'
import readProjectManifest from '@pnpm/read-project-manifest'
import { symlinkDirectRootDependency } from '@pnpm/symlink-dependency'
import {
  DependenciesField,
  DEPENDENCIES_FIELDS,
  DependencyManifest,
  ProjectManifest,
} from '@pnpm/types'
import {
  extendOptions,
  LinkOptions,
} from './options'
import path = require('path')
import normalize = require('normalize-path')
import pathAbsolute = require('path-absolute')
import R = require('ramda')

export default async function link (
  linkFromPkgs: Array<{alias: string, path: string} | string>,
  destModules: string,
  maybeOpts: LinkOptions & {
    linkToBin?: string
    dir: string
  }
) {
  const reporter = maybeOpts?.reporter
  if (reporter && typeof reporter === 'function') {
    streamParser.on('data', reporter)
  }
  const opts = await extendOptions(maybeOpts)
  const ctx = await getContextForSingleImporter(opts.manifest, {
    ...opts,
    extraBinPaths: [], // ctx.extraBinPaths is not needed, so this is fine
  })

  const importerId = getLockfileImporterId(ctx.lockfileDir, opts.dir)
  const currentLockfile = R.clone(ctx.currentLockfile)
  const linkedPkgs: Array<{path: string, manifest: DependencyManifest, alias: string}> = []
  const specsToUpsert = [] as PackageSpecObject[]

  for (const linkFrom of linkFromPkgs) {
    let linkFromPath: string
    let linkFromAlias: string | undefined
    if (typeof linkFrom === 'string') {
      linkFromPath = linkFrom
    } else {
      linkFromPath = linkFrom.path
      linkFromAlias = linkFrom.alias
    }
    const { manifest } = await readProjectManifest(linkFromPath) as { manifest: DependencyManifest }
    if (typeof linkFrom === 'string' && manifest.name === undefined) {
      throw new PnpmError('INVALID_PACKAGE_NAME', `Package in ${linkFromPath} must have a name field to be linked`)
    }

    specsToUpsert.push({
      alias: manifest.name,
      pref: getPref(manifest.name, manifest.name, manifest.version, {
        pinnedVersion: opts.pinnedVersion,
      }),
      saveType: (opts.targetDependenciesField ?? (ctx.manifest && guessDependencyType(manifest.name, ctx.manifest))) as DependenciesField,
    })

    const packagePath = normalize(path.relative(opts.dir, linkFromPath))
    const addLinkOpts = {
      linkedPkgName: linkFromAlias ?? manifest.name,
      manifest: ctx.manifest,
      packagePath,
    }
    addLinkToLockfile(ctx.currentLockfile.importers[importerId], addLinkOpts)
    addLinkToLockfile(ctx.wantedLockfile.importers[importerId], addLinkOpts)

    linkedPkgs.push({
      alias: linkFromAlias ?? manifest.name,
      manifest,
      path: linkFromPath,
    })
  }

  const updatedCurrentLockfile = pruneSharedLockfile(ctx.currentLockfile)

  const warn = (message: string) => logger.warn({ message, prefix: opts.dir })
  const updatedWantedLockfile = pruneSharedLockfile(ctx.wantedLockfile, { warn })

  await prune(
    [
      {
        binsDir: opts.binsDir,
        id: importerId,
        modulesDir: ctx.modulesDir,
        rootDir: opts.dir,
      },
    ],
    {
      currentLockfile,
      hoistedDependencies: ctx.hoistedDependencies,
      hoistedModulesDir: (opts.hoistPattern && ctx.hoistedModulesDir) ?? undefined,
      include: ctx.include,
      lockfileDir: opts.lockfileDir,
      publicHoistedModulesDir: (opts.publicHoistPattern && ctx.rootModulesDir) ?? undefined,
      registries: ctx.registries,
      skipped: ctx.skipped,
      storeController: opts.storeController,
      virtualStoreDir: ctx.virtualStoreDir,
      wantedLockfile: updatedCurrentLockfile,
    }
  )

  // Linking should happen after removing orphans
  // Otherwise would've been removed
  for (const { alias, manifest, path } of linkedPkgs) {
    // TODO: cover with test that linking reports with correct dependency types
    const stu = specsToUpsert.find((s) => s.alias === manifest.name)
    await symlinkDirectRootDependency(path, destModules, alias, {
      fromDependenciesField: stu?.saveType ?? opts.targetDependenciesField,
      linkedPackage: manifest,
      prefix: opts.dir,
    })
  }

  const linkToBin = maybeOpts?.linkToBin ?? path.join(destModules, '.bin')
  await linkBinsOfPackages(linkedPkgs.map((p) => ({ manifest: p.manifest, location: p.path })), linkToBin, {
    warn: (message: string) => logger.info({ message, prefix: opts.dir }),
  })

  let newPkg!: ProjectManifest
  if (opts.targetDependenciesField) {
    newPkg = await updateProjectManifestObject(opts.dir, opts.manifest, specsToUpsert)
    for (const { alias } of specsToUpsert) {
      updatedWantedLockfile.importers[importerId].specifiers[alias] = getSpecFromPackageManifest(newPkg, alias)
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
      wantedLockfileDir: ctx.lockfileDir,
      ...lockfileOpts,
    })
  } else {
    await writeCurrentLockfile(ctx.virtualStoreDir, updatedCurrentLockfile, lockfileOpts)
  }

  summaryLogger.debug({ prefix: opts.dir })

  if (reporter && typeof reporter === 'function') {
    streamParser.removeListener('data', reporter)
  }

  return newPkg
}

function addLinkToLockfile (
  projectSnapshot: ProjectSnapshot,
  opts: {
    linkedPkgName: string
    packagePath: string
    manifest?: ProjectManifest
  }
) {
  const id = `link:${opts.packagePath}`
  let addedTo: DependenciesField | undefined
  for (const depType of DEPENDENCIES_FIELDS) {
    if (!addedTo && opts.manifest?.[depType]?.[opts.linkedPkgName]) {
      addedTo = depType
      projectSnapshot[depType] = projectSnapshot[depType] ?? {}
      projectSnapshot[depType]![opts.linkedPkgName] = id
    } else if (projectSnapshot[depType]) {
      delete projectSnapshot[depType]![opts.linkedPkgName]
    }
  }

  // package.json might not be available when linking to global
  if (!opts.manifest) return

  const availableSpec = getSpecFromPackageManifest(opts.manifest, opts.linkedPkgName)
  if (availableSpec) {
    projectSnapshot.specifiers[opts.linkedPkgName] = availableSpec
  } else {
    delete projectSnapshot.specifiers[opts.linkedPkgName]
  }
}

export async function linkFromGlobal (
  pkgNames: string[],
  linkTo: string,
  maybeOpts: LinkOptions & {globalDir: string}
) {
  const reporter = maybeOpts?.reporter
  if (reporter && typeof reporter === 'function') {
    streamParser.on('data', reporter)
  }
  const opts = await extendOptions(maybeOpts)
  const globalPkgPath = pathAbsolute(maybeOpts.globalDir)
  const linkFromPkgs = pkgNames.map((pkgName) => path.join(globalPkgPath, 'node_modules', pkgName))
  const newManifest = await link(linkFromPkgs, path.join(linkTo, 'node_modules'), opts)

  if (reporter && typeof reporter === 'function') {
    streamParser.removeListener('data', reporter)
  }

  return newManifest
}

export async function linkToGlobal (
  linkFrom: string,
  maybeOpts: LinkOptions & {
    globalBin: string
    globalDir: string
  }
) {
  const reporter = maybeOpts?.reporter
  if (reporter && typeof reporter === 'function') {
    streamParser.on('data', reporter)
  }
  maybeOpts.lockfileDir = maybeOpts.lockfileDir ?? maybeOpts.globalDir
  const opts = await extendOptions(maybeOpts)
  const globalPkgPath = pathAbsolute(maybeOpts.globalDir)
  const newManifest = await link([linkFrom], path.join(globalPkgPath, 'node_modules'), {
    ...opts,
    dir: maybeOpts.globalDir,
    linkToBin: maybeOpts.globalBin,
  })

  if (reporter && typeof reporter === 'function') {
    streamParser.removeListener('data', reporter)
  }

  return newManifest
}
