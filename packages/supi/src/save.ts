import { packageJsonLogger } from '@pnpm/core-loggers'
import {
  DEPENDENCIES_FIELDS,
  DependenciesField,
  ImporterManifest,
} from '@pnpm/types'
import path = require('path')
import writePkg = require('write-pkg')

export default async function save (
  prefix: string,
  packageJson: ImporterManifest,
  packageSpecs: Array<{
    name: string,
    peer?: boolean,
    pref?: string,
    saveType?: DependenciesField | 'peerDependencies',
  }>,
  opts?: {
    dryRun?: boolean,
  }
): Promise<ImporterManifest> {
  const pkgJsonPath = path.join(prefix, 'package.json')

  packageSpecs.forEach((packageSpec) => {
    if (packageSpec.saveType) {
      const spec = packageSpec.pref || findSpec(packageSpec.name, packageJson as ImporterManifest)
      if (spec) {
        packageJson[packageSpec.saveType] = packageJson[packageSpec.saveType] || {}
        packageJson[packageSpec.saveType]![packageSpec.name] = spec
        DEPENDENCIES_FIELDS.filter((depField) => depField !== packageSpec.saveType).forEach((deptype) => {
          if (packageJson[deptype]) {
            delete packageJson[deptype]![packageSpec.name]
          }
        })
        if (packageSpec.peer === true) {
          packageJson.peerDependencies = packageJson.peerDependencies || {}
          packageJson.peerDependencies[packageSpec.name] = spec
        }
      }
    } else if (packageSpec.pref) {
      const usedDepType = guessDependencyType(packageSpec.name, packageJson as ImporterManifest) || 'dependencies'
      packageJson[usedDepType] = packageJson[usedDepType] || {}
      packageJson[usedDepType]![packageSpec.name] = packageSpec.pref
    }
  })

  if (!opts || opts.dryRun !== true) {
    await writePkg(pkgJsonPath, packageJson)
  }
  packageJsonLogger.debug({
    prefix,
    updated: packageJson,
  })
  return packageJson as ImporterManifest
}

function findSpec (depName: string, manifest: ImporterManifest): string | undefined {
  const foundDepType = guessDependencyType(depName, manifest)
  return foundDepType && manifest[foundDepType]![depName]
}

export function guessDependencyType (depName: string, manifest: ImporterManifest): DependenciesField | undefined {
  return DEPENDENCIES_FIELDS
    .find((depField) => Boolean(manifest[depField] && manifest[depField]![depName]))
}
