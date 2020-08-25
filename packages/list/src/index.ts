import { readProjectManifestOnly } from '@pnpm/read-project-manifest'
import { DependenciesField, Registries } from '@pnpm/types'
import dh from 'dependencies-hierarchy'
import createPackagesSearcher from './createPackagesSearcher'
import renderJson from './renderJson'
import renderParseable from './renderParseable'
import renderTree from './renderTree'
import { PackageDependencyHierarchy } from './types'
import R = require('ramda')

const DEFAULTS = {
  alwaysPrintRootPackage: true,
  depth: 0,
  long: false,
  registries: undefined,
  reportAs: 'tree' as const,
}

export async function forPackages (
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
    R.toPairs(await dh(projectPaths, {
      depth: opts.depth,
      include: maybeOpts?.include,
      lockfileDir: maybeOpts?.lockfileDir,
      registries: opts.registries,
      search,
    }))
      .map(async ([projectPath, dependenciesHierarchy]) => {
        const entryPkg = await readProjectManifestOnly(projectPath)
        return {
          name: entryPkg.name,
          version: entryPkg.version,

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
    search: Boolean(packages.length),
  })
}

export default async function (
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

  const pkgs = await Promise.all(
    R.toPairs(
      opts.depth === -1
        ? projectPaths.reduce((acc, projectPath) => {
          acc[projectPath] = {}
          return acc
        }, {})
        : await dh(projectPaths, {
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
  })
}

function getPrinter (reportAs: 'parseable' | 'tree' | 'json') {
  switch (reportAs) {
  case 'parseable': return renderParseable
  case 'json': return renderJson
  case 'tree': return renderTree
  }
}
