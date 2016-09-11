import requireJson from './fs/require_json'
import writeJson from './fs/write_json'
import {DependenciesType} from './get_save_type'

export default async function (pkgJsonPath: string, removedPackages: string[], saveType: DependenciesType) {
  const packageJson = requireJson(pkgJsonPath)
  packageJson[saveType] = packageJson[saveType]
  if (!packageJson[saveType]) return

  removedPackages.forEach(dependency => {
    delete packageJson[saveType][dependency]
  })

  return writeJson(pkgJsonPath, packageJson)
}
