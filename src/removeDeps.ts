import {ignoreCache as requireJson} from './fs/requireJson'
import writePkg = require('write-pkg')
import {DependenciesType} from './getSaveType'

export default async function (pkgJsonPath: string, removedPackages: string[], saveType: DependenciesType) {
  const packageJson = await requireJson(pkgJsonPath)
  packageJson[saveType] = packageJson[saveType]
  if (!packageJson[saveType]) return

  removedPackages.forEach(dependency => {
    delete packageJson[saveType][dependency]
  })

  return writePkg(pkgJsonPath, packageJson)
}
