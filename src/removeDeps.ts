import requireJson from './fs/requireJson'
import writeJson from './fs/writeJson'
import {DependenciesType} from './getSaveType'

export default async function (pkgJsonPath: string, removedPackages: string[], saveType: DependenciesType) {
  const packageJson = requireJson(pkgJsonPath)
  packageJson[saveType] = packageJson[saveType]
  if (!packageJson[saveType]) return

  removedPackages.forEach(dependency => {
    delete packageJson[saveType][dependency]
  })

  return writeJson(pkgJsonPath, packageJson)
}
