import '@total-typescript/ts-reset'

import path from 'node:path'

import {
  type PackageNode,
  createPackagesSearcher,
  buildDependenciesHierarchy,
  type DependenciesHierarchy,
} from '@pnpm/reviewing.dependencies-hierarchy'
import { safeReadProjectManifestOnly } from '@pnpm/read-project-manifest'
import type { PackageInfo, DependenciesField, Registries, PackageDependencyHierarchy } from '@pnpm/types'

import { renderJson } from './renderJson.js'
import { renderTree } from './renderTree.js'
import { renderParseable } from './renderParseable.js'
import { pruneDependenciesTrees } from './pruneTree.js'

export type { PackageNode } from '@pnpm/reviewing.dependencies-hierarchy'
export {
  renderJson,
  renderParseable,
  renderTree,
  type PackageDependencyHierarchy,
}

const DEFAULTS = {
  alwaysPrintRootPackage: true,
  depth: 0,
  long: false,
  registries: undefined,
  reportAs: 'tree' as const,
  showExtraneous: true,
} as const

export function flattenSearchedPackages(
  pkgs: PackageDependencyHierarchy[],
  opts: {
    lockfileDir: string
  }
): (DependenciesHierarchy & {
    name?: string | undefined;
    version?: string | undefined;
    path: string;
    private?: boolean | undefined;
  } & {
    depPath: string;
  })[] {
  const flattedPkgs: Array<PackageDependencyHierarchy & { depPath: string }> =
    []

  for (const pkg of pkgs) {
    _walker(
      [
        ...(pkg.optionalDependencies ?? []),
        ...(pkg.dependencies ?? []),
        ...(pkg.devDependencies ?? []),
        ...(pkg.unsavedDependencies ?? []),
      ],
      path.relative(opts.lockfileDir, pkg.path) || '.'
    )
  }

  return flattedPkgs

  function _walker(packages: (PackageNode | PackageInfo)[], depPath: string) {
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

export async function searchForPackages(
  packages: string[],
  projectPaths: string[],
  opts: {
    depth?: number | undefined
    lockfileDir: string
    include?: { [dependenciesField in DependenciesField]: boolean } | undefined
    onlyProjects?: boolean | undefined
    registries?: Registries | undefined
    modulesDir?: string | undefined
  }
) {
  const search = createPackagesSearcher(packages)

  return Promise.all(
    Object.entries(
      await buildDependenciesHierarchy(projectPaths, {
        depth: opts.depth,
        include: opts.include,
        lockfileDir: opts.lockfileDir,
        onlyProjects: opts.onlyProjects,
        registries: opts.registries,
        search,
        modulesDir: opts.modulesDir,
      })
    ).map(async ([projectPath, depsHierarchy]: [string, DependenciesHierarchy]): Promise<PackageDependencyHierarchy> => {
      const entryPkg = (await safeReadProjectManifestOnly(projectPath)) ?? {}

      return {
        name: entryPkg.name,
        version: entryPkg.version,

        path: projectPath,
        ...depsHierarchy,
      }
    })
  )
}

export async function listForPackages(
  packages: string[],
  projectPaths: string[],
  maybeOpts: {
    alwaysPrintRootPackage?: boolean
    depth?: number | undefined
    lockfileDir: string
    long?: boolean | undefined
    include?: { [dependenciesField in DependenciesField]: boolean } | undefined
    onlyProjects?: boolean | undefined
    reportAs?: 'parseable' | 'tree' | 'json' | undefined
    registries?: Registries | undefined
    modulesDir?: string | undefined
  }
) {
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

export async function list(
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
) {
  const opts = { ...DEFAULTS, ...maybeOpts }

  const pkgs = await Promise.all(
    Object.entries(
      opts.depth === -1
        ? projectPaths.reduce(
          (acc, projectPath) => {
            acc[projectPath] = {}
            return acc
          },
          {} as Record<string, DependenciesHierarchy>
        )
        : await buildDependenciesHierarchy(projectPaths, {
          depth: opts.depth,
          include: maybeOpts?.include,
          lockfileDir: maybeOpts?.lockfileDir,
          onlyProjects: maybeOpts?.onlyProjects,
          registries: opts.registries,
          modulesDir: opts.modulesDir,
        })
    ).map(async ([projectPath, dependenciesHierarchy]) => {
      const entryPkg = (await safeReadProjectManifestOnly(projectPath)) ?? {}
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

function getPrinter(reportAs: 'parseable' | 'tree' | 'json') {
  switch (reportAs) {
    case 'parseable':
      return renderParseable
    case 'json':
      return renderJson
    case 'tree':
      return renderTree
  }
}
