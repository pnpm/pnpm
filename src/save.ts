import loadJsonFile = require('load-json-file')
import writePkg = require('write-pkg')
import {DependenciesType, dependenciesTypes} from './getSaveType'
import {Package} from './types'
import {PackageSpec} from 'package-store'
import {manifestLogger} from 'pnpm-logger'

export default async function save (
  pkgJsonPath: string,
  packageSpecs: ({
    name: string,
    saveSpec: string,
  })[],
  saveType?: DependenciesType
): Promise<Package> {
  // Read the latest version of package.json to avoid accidental overwriting
  const packageJson = await loadJsonFile(pkgJsonPath)
  if (saveType) {
    packageJson[saveType] = packageJson[saveType] || {}
    packageSpecs.forEach(dependency => {
      packageJson[saveType][dependency.name] = dependency.saveSpec
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
      packageJson[usedDepType][dependency.name] = dependency.saveSpec
    })
  }

  await writePkg(pkgJsonPath, packageJson)
  manifestLogger.debug({ updated: packageJson })
  return packageJson
}

function guessDependencyType (depName: string, pkg: Package): DependenciesType | undefined {
  return dependenciesTypes
    .find(deptype => Boolean(pkg[deptype] && pkg[deptype]![depName]))
}
