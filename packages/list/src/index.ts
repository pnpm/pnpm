import { readImporterManifestOnly } from '@pnpm/read-importer-manifest'
import { DependenciesField, Registries } from '@pnpm/types'
import npa = require('@zkochan/npm-package-arg')
import dh, {
  forPackages as dhForPackages,
  PackageSelector,
} from 'dependencies-hierarchy'
import R =  require('ramda')
import renderJson from './renderJson'
import renderParseable from './renderParseable'
import renderTree from './renderTree'
import { PackageDependencyHierarchy } from './types'

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
    alwaysPrintRootPackage?: boolean,
    depth?: number,
    lockfileDirectory: string,
    long?: boolean,
    include?: { [dependenciesField in DependenciesField]: boolean },
    reportAs?: 'parseable' | 'tree' | 'json',
    registries?: Registries,
  },
) {
  const opts = { ...DEFAULTS, ...maybeOpts }

  const searched: PackageSelector[] = packages.map((arg) => {
    const parsed = npa(arg)
    if (parsed.raw === parsed.name) {
      return parsed.name
    }
    if (parsed.type !== 'version' && parsed.type !== 'range') {
      throw new Error(`Invalid argument - ${arg}. List can search only by version or range`)
    }
    return {
      name: parsed.name,
      range: parsed.fetchSpec,
    }
  })

  const pkgs = await Promise.all(
    R.toPairs(await dhForPackages(searched, projectPaths, {
      depth: opts.depth,
      include: maybeOpts && maybeOpts.include,
      lockfileDirectory: maybeOpts && maybeOpts.lockfileDirectory,
      registries: opts.registries,
    }))
    .map(async ([projectPath, dependenciesHierarchy]) => {
      const entryPkg = await readImporterManifestOnly(projectPath)
      return {
        path: projectPath,
        ...entryPkg,
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
    alwaysPrintRootPackage?: boolean,
    depth?: number,
    lockfileDirectory: string,
    long?: boolean,
    include?: { [dependenciesField in DependenciesField]: boolean },
    reportAs?: 'parseable' | 'tree' | 'json',
    registries?: Registries,
  },
) {
  const opts = { ...DEFAULTS, ...maybeOpts }


  const pkgs = await Promise.all(
    R.toPairs(
      opts.depth === -1
      ? {}
      : await dh(projectPaths, {
        depth: opts.depth,
        include: maybeOpts && maybeOpts.include,
        lockfileDirectory: maybeOpts && maybeOpts.lockfileDirectory,
        registries: opts.registries,
      })
    )
    .map(async ([projectPath, dependenciesHierarchy]) => {
      const entryPkg = await readImporterManifestOnly(projectPath)
      return {
        path: projectPath,
        ...entryPkg,
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
