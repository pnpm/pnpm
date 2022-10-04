import path from 'path'
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
import { pruneSharedLockfile } from '@pnpm/prune-lockfile'
import readProjectManifest from '@pnpm/read-project-manifest'
import { symlinkDirectRootDependency } from '@pnpm/symlink-dependency'
import {
  DependenciesField,
  DEPENDENCIES_FIELDS,
  DependencyManifest,
  ProjectManifest,
} from '@pnpm/types'
import normalize from 'normalize-path'
import {
  extendOptions,
  LinkOptions,
} from './options'

type LinkFunctionOptions = LinkOptions & {
  linkToBin?: string
  dir: string
}

export { LinkFunctionOptions }

export default async function link (
  linkFromPkgs: Array<{ alias: string, path: string } | string>,
  destModules: string,
  maybeOpts: LinkFunctionOptions
) {
  const reporter = maybeOpts?.reporter
  if ((reporter != null) && typeof reporter === 'function') {
    streamParser.on('data', reporter)
  }
  const opts = await extendOptions(maybeOpts)
  const ctx = await getContextForSingleImporter(opts.manifest, {
    ...opts,
    extraBinPaths: [], // ctx.extraBinPaths is not needed, so this is fine
  }, true)

  const importerId = getLockfileImporterId(ctx.lockfileDir, opts.dir)
  const linkedPkgs: Array<{ path: string, manifest: DependencyManifest, alias: string }> = []
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
    extraNodePaths: ctx.extraNodePaths,
    preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
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
  const lockfileOpts = { forceSharedFormat: opts.forceSharedLockfile, useGitBranchLockfile: opts.useGitBranchLockfile, mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles }
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

  if ((reporter != null) && typeof reporter === 'function') {
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
    } else if (projectSnapshot[depType] != null) {
      delete projectSnapshot[depType]![opts.linkedPkgName]
    }
  }

  // package.json might not be available when linking to global
  if (opts.manifest == null) return

  const availableSpec = getSpecFromPackageManifest(opts.manifest, opts.linkedPkgName)
  if (availableSpec) {
    projectSnapshot.specifiers[opts.linkedPkgName] = availableSpec
  } else {
    delete projectSnapshot.specifiers[opts.linkedPkgName]
  }
}
