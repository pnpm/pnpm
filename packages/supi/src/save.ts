import {PackageJson} from '@pnpm/types'
import loadJsonFile = require('load-json-file')
import writePkg = require('write-pkg')
import {DependenciesType, dependenciesTypes} from './getSaveType'
import {packageJsonLogger} from './loggers'

export default async function save (
  pkgJsonPath: string,
  packageSpecs: Array<{
    name: string,
    pref: string,
  }>,
  saveType?: DependenciesType,
): Promise<PackageJson> {
  // Read the latest version of package.json to avoid accidental overwriting
  let packageJson: object
  try {
    packageJson = await loadJsonFile(pkgJsonPath)
  } catch (err) {
    if (err['code'] !== 'ENOENT') throw err // tslint:disable-line:no-string-literal
    packageJson = {}
  }
  if (saveType) {
    packageJson[saveType] = packageJson[saveType] || {}
    packageSpecs.forEach((dependency) => {
      packageJson[saveType][dependency.name] = dependency.pref
      dependenciesTypes.filter((deptype) => deptype !== saveType).forEach((deptype) => {
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

  await writePkg(pkgJsonPath, packageJson)
  packageJsonLogger.debug({ updated: packageJson })
  return packageJson as PackageJson
}

function guessDependencyType (depName: string, pkg: PackageJson): DependenciesType | undefined {
  return dependenciesTypes
    .find((deptype) => Boolean(pkg[deptype] && pkg[deptype]![depName]))
}
