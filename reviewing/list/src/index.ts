import path from 'path'
import { readWantedLockfile } from '@pnpm/lockfile.fs'
import { safeReadProjectManifestOnly } from '@pnpm/read-project-manifest'
import { type DependenciesField, type Registries, type Finder } from '@pnpm/types'
import { type PackageNode, buildDependenciesHierarchy, type DependenciesHierarchy, createPackagesSearcher, buildWhyTrees, type ImporterInfo } from '@pnpm/reviewing.dependencies-hierarchy'
import { renderJson } from './renderJson.js'
import { renderParseable } from './renderParseable.js'
import { renderTree } from './renderTree.js'
import { renderWhyTree, renderWhyJson, renderWhyParseable } from './renderWhyTree.js'
import { type PackageDependencyHierarchy } from './types.js'

export type { PackageNode } from '@pnpm/reviewing.dependencies-hierarchy'
export { renderJson, renderParseable, renderTree, type PackageDependencyHierarchy }

const DEFAULTS = {
  alwaysPrintRootPackage: true,
  depth: 0,
  long: false,
  registries: undefined,
  reportAs: 'tree' as const,
  showExtraneous: true,
}

export interface FlattenedSearchPackage extends PackageDependencyHierarchy {
  depPath: string
}

export function flattenSearchedPackages (pkgs: PackageDependencyHierarchy[], opts: {
  lockfileDir: string
}): FlattenedSearchPackage[] {
  const flattedPkgs: FlattenedSearchPackage[] = []
  for (const pkg of pkgs) {
    _walker([
      ...(pkg.optionalDependencies ?? []),
      ...(pkg.dependencies ?? []),
      ...(pkg.devDependencies ?? []),
      ...(pkg.unsavedDependencies ?? []),
    ], path.relative(opts.lockfileDir, pkg.path) || '.')
  }

  return flattedPkgs

  function _walker (packages: PackageNode[], depPath: string): void {
    for (const pkg of packages) {
      const nextDepPath = `${depPath} > ${pkg.name}@${pkg.version}`
      if (pkg.dependencies?.length) {
        _walker(pkg.dependencies, nextDepPath)
      } else {
        flattedPkgs.push({
          depPath: nextDepPath,
          ...pkg,
        })
      }
    }
  }
}

export async function searchForPackages (
  packages: string[],
  projectPaths: string[],
  opts: {
    depth: number
    excludePeerDependencies?: boolean
    lockfileDir: string
    checkWantedLockfileOnly?: boolean
    include?: { [dependenciesField in DependenciesField]: boolean }
    onlyProjects?: boolean
    registries?: Registries
    modulesDir?: string
    virtualStoreDirMaxLength: number
    finders?: Finder[]
  }
): Promise<PackageDependencyHierarchy[]> {
  const search = createPackagesSearcher(packages, opts.finders)

  return Promise.all(
    Object.entries(await buildDependenciesHierarchy(projectPaths, {
      depth: opts.depth,
      excludePeerDependencies: opts.excludePeerDependencies,
      include: opts.include,
      lockfileDir: opts.lockfileDir,
      checkWantedLockfileOnly: opts.checkWantedLockfileOnly,
      onlyProjects: opts.onlyProjects,
      registries: opts.registries,
      search,
      showDedupedSearchMatches: true,
      modulesDir: opts.modulesDir,
      virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    }))
      .map(async ([projectPath, buildDependenciesHierarchy]) => {
        const entryPkg = await safeReadProjectManifestOnly(projectPath) ?? {}
        return {
          name: entryPkg.name,
          version: entryPkg.version,
          private: entryPkg.private,

          path: projectPath,
          ...buildDependenciesHierarchy,
        } as PackageDependencyHierarchy
      })
  )
}

export async function listForPackages (
  packages: string[],
  projectPaths: string[],
  maybeOpts: {
    alwaysPrintRootPackage?: boolean
    depth?: number
    lockfileDir: string
    checkWantedLockfileOnly?: boolean
    long?: boolean
    include?: { [dependenciesField in DependenciesField]: boolean }
    onlyProjects?: boolean
    reportAs?: 'parseable' | 'tree' | 'json'
    registries?: Registries
    modulesDir?: string
    virtualStoreDirMaxLength: number
    finders?: Finder[]
    showSummary?: boolean
  }
): Promise<string> {
  const opts = { ...DEFAULTS, ...maybeOpts }

  const pkgs = await searchForPackages(packages, projectPaths, opts)

  const print = getPrinter(opts.reportAs)
  return print(pkgs, {
    alwaysPrintRootPackage: opts.alwaysPrintRootPackage,
    depth: opts.depth,
    long: opts.long,
    search: Boolean(packages.length),
    showExtraneous: opts.showExtraneous,
    showSummary: opts.showSummary,
  })
}

export async function list (
  projectPaths: string[],
  maybeOpts: {
    alwaysPrintRootPackage?: boolean
    depth?: number
    excludePeerDependencies?: boolean
    lockfileDir: string
    checkWantedLockfileOnly?: boolean
    long?: boolean
    include?: { [dependenciesField in DependenciesField]: boolean }
    onlyProjects?: boolean
    reportAs?: 'parseable' | 'tree' | 'json'
    registries?: Registries
    showExtraneous?: boolean
    modulesDir?: string
    virtualStoreDirMaxLength: number
    finders?: Finder[]
    showSummary?: boolean
  }
): Promise<string> {
  const opts = { ...DEFAULTS, ...maybeOpts }

  const pkgs = await Promise.all(
    Object.entries(
      opts.depth === -1
        ? projectPaths.reduce((acc, projectPath) => {
          acc[projectPath] = {}
          return acc
        }, {} as Record<string, DependenciesHierarchy>)
        : await buildDependenciesHierarchy(projectPaths, {
          depth: opts.depth,
          excludePeerDependencies: maybeOpts?.excludePeerDependencies,
          include: maybeOpts?.include,
          lockfileDir: maybeOpts?.lockfileDir,
          checkWantedLockfileOnly: maybeOpts?.checkWantedLockfileOnly,
          onlyProjects: maybeOpts?.onlyProjects,
          registries: opts.registries,
          modulesDir: opts.modulesDir,
          virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
        })
    )
      .map(async ([projectPath, dependenciesHierarchy]) => {
        const entryPkg = await safeReadProjectManifestOnly(projectPath) ?? {}
        return {
          name: entryPkg.name,
          version: entryPkg.version,
          private: entryPkg.private,

          path: projectPath,
          ...dependenciesHierarchy,
        } as PackageDependencyHierarchy
      })
  )

  const print = getPrinter(opts.reportAs)
  return print(pkgs, {
    alwaysPrintRootPackage: opts.alwaysPrintRootPackage,
    depth: opts.depth,
    long: opts.long,
    search: false,
    showExtraneous: opts.showExtraneous,
    showSummary: opts.showSummary,
  })
}

type Printer = (packages: PackageDependencyHierarchy[], opts: {
  alwaysPrintRootPackage: boolean
  depth: number
  long: boolean
  search: boolean
  showExtraneous: boolean
  showSummary?: boolean
}) => Promise<string>

function getPrinter (reportAs: 'parseable' | 'tree' | 'json'): Printer {
  switch (reportAs) {
  case 'parseable': return renderParseable
  case 'json': return renderJson
  case 'tree': return renderTree
  }
}

export async function whyForPackages (
  packages: string[],
  projectPaths: string[],
  opts: {
    lockfileDir: string
    checkWantedLockfileOnly?: boolean
    include?: { [dependenciesField in DependenciesField]: boolean }
    reportAs?: 'parseable' | 'tree' | 'json'
    modulesDir?: string
    virtualStoreDirMaxLength: number
    finders?: Finder[]
  }
): Promise<string> {
  const reportAs = opts.reportAs ?? 'tree'

  // Build importer info map by reading the lockfile to discover all importers,
  // then reading each project's package.json for name/version.
  const importerInfoMap = new Map<string, ImporterInfo>()
  const lockfile = await readWantedLockfile(opts.lockfileDir, { ignoreIncompatible: false })
  if (lockfile) {
    const importerIds = Object.keys(lockfile.importers)
    const manifests = await Promise.all(
      importerIds.map((importerId) => safeReadProjectManifestOnly(path.join(opts.lockfileDir, importerId)))
    )
    for (let i = 0; i < importerIds.length; i++) {
      const importerId = importerIds[i]
      const manifest = manifests[i]
      importerInfoMap.set(importerId, {
        name: manifest?.name ?? (importerId === '.' ? 'the root project' : importerId),
        version: manifest?.version ?? '',
      })
    }
  }

  const results = await buildWhyTrees(packages, projectPaths, {
    lockfileDir: opts.lockfileDir,
    include: opts.include,
    modulesDir: opts.modulesDir,
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    checkWantedLockfileOnly: opts.checkWantedLockfileOnly,
    finders: opts.finders,
    importerInfoMap,
  })

  switch (reportAs) {
  case 'json': return renderWhyJson(results)
  case 'parseable': return renderWhyParseable(results)
  case 'tree': return renderWhyTree(results)
  }
}
