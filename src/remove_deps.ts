import requireJson from './fs/require_json'
import writeJson from './fs/write_json'

export default (pkg, removedPackages, saveType) => {
  const packageJson = requireJson(pkg.path)
  packageJson[saveType] = packageJson[saveType]
  if (!packageJson[saveType]) return Promise.resolve()

  removedPackages.forEach(dependency => {
    delete packageJson[saveType][dependency]
  })

  return writeJson(pkg.path, packageJson)
}
