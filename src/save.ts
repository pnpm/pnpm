import loadJsonFile = require('load-json-file')
import writePkg = require('write-pkg')
import {DependenciesType, dependenciesTypes} from './getSaveType'
import {PackageJson} from '@pnpm/types'
import {packageJsonLogger} from './loggers'

export default async function save (
  pkgJsonPath: string,
  packageSpecs: ({
    name: string,
    pref: string,
  })[],
  saveType?: DependenciesType
): Promise<PackageJson> {
  // Read the latest version of package.json to avoid accidental overwriting
  const packageJson = await loadJsonFile(pkgJsonPath)
  if (saveType) {
    packageJson[saveType] = packageJson[saveType] || {}
    packageSpecs.forEach(dependency => {
      packageJson[saveType][dependency.name] = dependency.pref
      dependenciesTypes.filter(deptype => deptype !== saveType).forEach(deptype => {
        if (packageJson[deptype]) {
          delete packageJson[deptype][dependency.name]
        }
      })
    })
  } else {
    packageSpecs.forEach(dependency => {
      const usedDepType = guessDependencyType(dependency.name, packageJson) || 'dependencies'
      packageJson[usedDepType] = packageJson[usedDepType] || {}
      packageJson[usedDepType][dependency.name] = dependency.pref
    })
  }

  await writePkg(pkgJsonPath, packageJson)
  packageJsonLogger.debug({ updated: packageJson })
  return packageJson
}

function guessDependencyType (depName: string, pkg: PackageJson): DependenciesType | undefined {
  return dependenciesTypes
    .find(deptype => Boolean(pkg[deptype] && pkg[deptype]![depName]))
}
