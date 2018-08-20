import {PackageJson} from '@pnpm/types'
import {
  DependenciesType,
  dependenciesTypes,
  packageJsonLogger,
} from '@pnpm/utils'
import loadJsonFile = require('load-json-file')
import path = require('path')
import writePkg = require('write-pkg')

export default async function save (
  prefix: string,
  packageSpecs: Array<{
    name: string,
    pref?: string,
    saveType?: DependenciesType,
  }>,
): Promise<PackageJson> {
  // Read the latest version of package.json to avoid accidental overwriting
  let packageJson: object
  const pkgJsonPath = path.join(prefix, 'package.json')
  try {
    packageJson = await loadJsonFile(pkgJsonPath)
  } catch (err) {
    if (err['code'] !== 'ENOENT') throw err // tslint:disable-line:no-string-literal
    packageJson = {}
  }
  packageSpecs.forEach((packageSpec) => {
    if (packageSpec.saveType) {
      const saveType = packageSpec.saveType
      packageJson[packageSpec.saveType] = packageJson[packageSpec.saveType] || {}
      packageSpecs.forEach((dependency) => {
        packageJson[saveType][dependency.name] = dependency.pref || findSpec(dependency.name, packageJson as PackageJson)
        dependenciesTypes.filter((deptype) => deptype !== packageSpec.saveType).forEach((deptype) => {
          if (packageJson[deptype]) {
            delete packageJson[deptype][dependency.name]
          }
        })
      })
    } else {
      packageSpecs.forEach((dependency) => {
        const usedDepType = guessDependencyType(dependency.name, packageJson as PackageJson) || 'dependencies'
        packageJson[usedDepType] = packageJson[usedDepType] || {}
        packageJson[usedDepType][dependency.name] = dependency.pref
      })
    }
  })

  await writePkg(pkgJsonPath, packageJson)
  packageJsonLogger.debug({
    prefix,
    updated: packageJson,
  })
  return packageJson as PackageJson
}

function findSpec (depName: string, pkg: PackageJson): string | undefined {
  const foundDepType = guessDependencyType(depName, pkg)
  return foundDepType && pkg[foundDepType]![depName]
}

export function guessDependencyType (depName: string, pkg: PackageJson): DependenciesType | undefined {
  return dependenciesTypes
    .find((deptype) => Boolean(pkg[deptype] && pkg[deptype]![depName]))
}
