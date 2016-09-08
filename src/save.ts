import requireJson from './fs/require_json'
import writeJson from './fs/write_json'
import sortedObject = require('sorted-object')

export default function save (pkg, installedPackages, saveType, useExactVersion) {
  // Read the latest version of package.json to avoid accidental overwriting
  const packageJson = requireJson(pkg.path, { ignoreCache: true })
  packageJson[saveType] = packageJson[saveType] || {}
  installedPackages.forEach(dependency => {
    const semverCharacter = useExactVersion ? '' : '^'
    packageJson[saveType][dependency.spec.name] = semverCharacter + dependency.version
  })
  packageJson[saveType] = sortedObject(packageJson[saveType])

  return writeJson(pkg.path, packageJson)
}
