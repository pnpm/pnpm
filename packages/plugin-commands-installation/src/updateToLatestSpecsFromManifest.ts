import { filterDependenciesByType, getAllDependenciesFromManifest } from '@pnpm/manifest-utils'
import { IncludedDependencies, ProjectManifest } from '@pnpm/types'
import R = require('ramda')
import getVerSelType = require('version-selector-type')

export default function updateToLatestSpecsFromManifest (manifest: ProjectManifest, include: IncludedDependencies) {
  const allDeps = filterDependenciesByType(manifest, include)
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

export function createLatestSpecs (specs: string[], manifest: ProjectManifest) {
  const allDeps = getAllDependenciesFromManifest(manifest)
  return specs
    .filter((selector) => selector.includes('@', 1)
      ? allDeps[selector.substr(0, selector.indexOf('@', 1))]
      : allDeps[selector]
    )
    .map((selector) => {
      if (selector.includes('@', 1)) {
        return selector
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
