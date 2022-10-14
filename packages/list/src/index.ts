import { readProjectManifestOnly } from '@pnpm/read-project-manifest'
import { DependenciesField, Registries } from '@pnpm/types'
import { buildDependenciesHierarchy } from 'dependencies-hierarchy'
import { createPackagesSearcher } from './createPackagesSearcher'
import { renderJson } from './renderJson'
import { renderParseable } from './renderParseable'
import { renderTree } from './renderTree'
import { PackageDependencyHierarchy } from './types'

const DEFAULTS = {
  alwaysPrintRootPackage: true,
  depth: 0,
  long: false,
  registries: undefined,
  reportAs: 'tree' as const,
  showExtraneous: true,
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
    reportAs?: 'parseable' | 'tree' | 'json'
    registries?: Registries
  }
) {
  const opts = { ...DEFAULTS, ...maybeOpts }

  const search = createPackagesSearcher(packages)

  const pkgs = await Promise.all(
    Object.entries(await buildDependenciesHierarchy(projectPaths, {
      depth: opts.depth,
      include: maybeOpts?.include,
      lockfileDir: maybeOpts?.lockfileDir,
      registries: opts.registries,
      search,
    }))
      .map(async ([projectPath, buildDependenciesHierarchy]) => {
        const entryPkg = await readProjectManifestOnly(projectPath)
        return {
          name: entryPkg.name,
          version: entryPkg.version,

          path: projectPath,
          ...buildDependenciesHierarchy,
        } as PackageDependencyHierarchy
      })
  )

  const print = getPrinter(opts.reportAs)
  return print(pkgs, {
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
    reportAs?: 'parseable' | 'tree' | 'json'
    registries?: Registries
    showExtraneous?: boolean
  }
) {
  const opts = { ...DEFAULTS, ...maybeOpts }

  const pkgs = await Promise.all(
    Object.entries(
      opts.depth === -1
        ? projectPaths.reduce((acc, projectPath) => {
          acc[projectPath] = {}
          return acc
        }, {})
        : await buildDependenciesHierarchy(projectPaths, {
          depth: opts.depth,
          include: maybeOpts?.include,
          lockfileDir: maybeOpts?.lockfileDir,
          registries: opts.registries,
        })
    )
      .map(async ([projectPath, dependenciesHierarchy]) => {
        const entryPkg = await readProjectManifestOnly(projectPath)
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

function getPrinter (reportAs: 'parseable' | 'tree' | 'json') {
  switch (reportAs) {
  case 'parseable': return renderParseable
  case 'json': return renderJson
  case 'tree': return renderTree
  }
}
