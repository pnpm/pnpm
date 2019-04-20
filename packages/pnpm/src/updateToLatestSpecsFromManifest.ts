import { ImporterManifest } from '@pnpm/types'
import { getAllDependenciesFromPackage } from '@pnpm/utils'
import R = require('ramda')
import getVerSelType = require('version-selector-type')

export default function (manifest: ImporterManifest) {
  const allDeps = getAllDependenciesFromPackage(manifest)
  const updateSpecs = []
  for (const [depName, depVersion] of R.toPairs(allDeps)) {
    if (depVersion.startsWith('npm:')) {
      updateSpecs.push(`${depName}@${depVersion.substr(0, depVersion.lastIndexOf('@'))}@latest`)
    } else {
      const selector = getVerSelType(depVersion)
      if (!selector) continue
      updateSpecs.push(`${depName}@latest`)
    }
  }
  return updateSpecs
}
