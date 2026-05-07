import path from 'node:path'

import { createMatcher } from '@pnpm/config.matcher'
import { type DependencyNode, renderJson, renderParseable, renderTree } from '@pnpm/deps.inspection.list'
import {
  getGlobalPackageDetails,
  scanGlobalPackages,
} from '@pnpm/global.packages'
import { lexCompare } from '@pnpm/util.lex-comparator'

export function findGlobalInstallDirs (globalPkgDir: string, params: string[]): string[] {
  const packages = scanGlobalPackages(globalPkgDir)
  const matches = params.length > 0 ? createMatcher(params) : () => true
  const installDirs = new Set<string>()
  for (const pkg of packages) {
    for (const alias of Object.keys(pkg.dependencies)) {
      if (matches(alias)) {
        installDirs.add(pkg.installDir)
        break
      }
    }
  }
  return [...installDirs]
}

export interface ListGlobalPackagesOptions {
  long?: boolean
  reportAs?: 'parseable' | 'tree' | 'json'
}

export async function listGlobalPackages (
  globalPkgDir: string,
  params: string[],
  opts: ListGlobalPackagesOptions = {}
): Promise<string> {
  const reportAs = opts.reportAs ?? 'tree'
  const long = opts.long ?? false
  const packages = scanGlobalPackages(globalPkgDir)
  const allDetails = await Promise.all(packages.map((pkg) => getGlobalPackageDetails(pkg)))
  const matches = params.length > 0 ? createMatcher(params) : () => true
  const dependencies: DependencyNode[] = []
  for (let i = 0; i < packages.length; i++) {
    const installDir = packages[i].installDir
    for (const installed of allDetails[i]) {
      if (!matches(installed.alias)) continue
      dependencies.push({
        alias: installed.alias,
        name: installed.manifest.name,
        version: installed.version,
        path: path.join(installDir, 'node_modules', installed.alias),
        isPeer: false,
        isSkipped: false,
        isMissing: false,
      })
    }
  }
  dependencies.sort((a, b) => lexCompare(a.alias, b.alias))

  if (dependencies.length === 0) {
    if (reportAs === 'json') {
      return JSON.stringify([{ path: globalPkgDir, private: true, dependencies: {} }], null, 2)
    }
    if (reportAs === 'parseable') {
      return globalPkgDir
    }
    return params.length > 0
      ? 'No matching global packages found'
      : 'No global packages found'
  }

  const hierarchy = [{
    path: globalPkgDir,
    private: true,
    dependencies,
  }]

  switch (reportAs) {
    case 'json':
      return renderJson(hierarchy, { depth: 0, long, search: false })
    case 'parseable':
      return renderParseable(hierarchy, { depth: 0, long, alwaysPrintRootPackage: true, search: false })
    case 'tree':
      return renderTree(hierarchy, {
        alwaysPrintRootPackage: false,
        depth: 0,
        long,
        search: false,
        showExtraneous: false,
      })
  }
}
