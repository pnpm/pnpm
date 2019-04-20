import { ImporterManifest, IncludedDependencies } from '@pnpm/types'
import { getAllDependenciesFromPackage } from '@pnpm/utils'
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
      updateSpecs.push(`${depName}@${removeVersionFromSpec(depVersion)}@latest`)
    } else {
      const selector = getVerSelType(depVersion)
      if (!selector) continue
      updateSpecs.push(`${depName}@latest`)
    }
  }
  return updateSpecs
}

export function createLatestSpecs (specs: string[], manifest: ImporterManifest) {
  const allDeps = getAllDependenciesFromPackage(manifest)
  return specs.map((selector) => {
    if (selector.includes('@', 1)) {
      return selector
    }
    if (!allDeps[selector]) {
      return `${selector}@latest`
    }
    if (allDeps[selector].startsWith('npm:')) {
      return `${selector}@${removeVersionFromSpec(allDeps[selector])}@latest`
    }
    if (!getVerSelType(allDeps[selector])) {
      return selector
    }
    return `${selector}@latest`
  })
}

function removeVersionFromSpec (spec: string) {
  return spec.substr(0, spec.lastIndexOf('@'))
}
