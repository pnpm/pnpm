import { ImporterManifest, IncludedDependencies } from '@pnpm/types'
import R = require('ramda')
import getVerSelType = require('version-selector-type')

export default function (manifest: ImporterManifest, include: IncludedDependencies) {
  const allDeps = {
    ...(include.devDependencies ? manifest.devDependencies : {}),
    ...(include.dependencies ? manifest.dependencies : {}),
    ...(include.optionalDependencies ? manifest.optionalDependencies : {}),
  } as { [name: string]: string }
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
