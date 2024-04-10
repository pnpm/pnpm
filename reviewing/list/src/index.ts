import path from 'path'
import { safeReadProjectManifestOnly } from '@pnpm/read-project-manifest'
import { type DependenciesField, type Registries } from '@pnpm/types'
import { type PackageNode, buildDependenciesHierarchy, type DependenciesHierarchy, createPackagesSearcher } from '@pnpm/reviewing.dependencies-hierarchy'
import { renderJson } from './renderJson'
import { renderParseable } from './renderParseable'
import { renderTree } from './renderTree'
import { type PackageDependencyHierarchy } from './types'
import { pruneDependenciesTrees } from './pruneTree'

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
    lockfileDir: string
    include?: { [dependenciesField in DependenciesField]: boolean }
    onlyProjects?: boolean
    registries?: Registries
    modulesDir?: string
  }
): Promise<PackageDependencyHierarchy[]> {
  const search = createPackagesSearcher(packages)

  return Promise.all(
    Object.entries(await buildDependenciesHierarchy(projectPaths, {
      depth: opts.depth,
      include: opts.include,
      lockfileDir: opts.lockfileDir,
      onlyProjects: opts.onlyProjects,
      registries: opts.registries,
      search,
      modulesDir: opts.modulesDir,
    }))
      .map(async ([projectPath, buildDependenciesHierarchy]) => {
        const entryPkg = await safeReadProjectManifestOnly(projectPath) ?? {}
        return {
          name: entryPkg.name,
          version: entryPkg.version,

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
    long?: boolean
    include?: { [dependenciesField in DependenciesField]: boolean }
    onlyProjects?: boolean
    reportAs?: 'parseable' | 'tree' | 'json'
    registries?: Registries
    modulesDir?: string
  }
): Promise<string> {
  const opts = { ...DEFAULTS, ...maybeOpts }

  const pkgs = await searchForPackages(packages, projectPaths, opts)

  const prunedPkgs = pruneDependenciesTrees(pkgs ?? null, 10)

  const print = getPrinter(opts.reportAs)
  return print(prunedPkgs, {
    alwaysPrintRootPackage: opts.alwaysPrintRootPackage,
    depth: opts.depth,
    long: opts.long,
    search: Boolean(packages.length),
    showExtraneous: opts.showExtraneous,
  })
}

export async function list (
  projectPaths: string[],
  maybeOpts: {
    alwaysPrintRootPackage?: boolean
    depth?: number
    lockfileDir: string
    long?: boolean
    include?: { [dependenciesField in DependenciesField]: boolean }
    onlyProjects?: boolean
    reportAs?: 'parseable' | 'tree' | 'json'
    registries?: Registries
    showExtraneous?: boolean
    modulesDir?: string
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
          include: maybeOpts?.include,
          lockfileDir: maybeOpts?.lockfileDir,
          onlyProjects: maybeOpts?.onlyProjects,
          registries: opts.registries,
          modulesDir: opts.modulesDir,
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
  })
}

type Printer = (packages: PackageDependencyHierarchy[], opts: {
  alwaysPrintRootPackage: boolean
  depth: number
  long: boolean
  search: boolean
  showExtraneous: boolean
}) => Promise<string>

function getPrinter (reportAs: 'parseable' | 'tree' | 'json'): Printer {
  switch (reportAs) {
  case 'parseable': return renderParseable
  case 'json': return renderJson
  case 'tree': return renderTree
  }
}
