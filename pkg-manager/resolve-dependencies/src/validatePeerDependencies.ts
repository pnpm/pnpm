import { PnpmError } from '@pnpm/error'
import { type ProjectManifest } from '@pnpm/types'
import { isValidPeerRange } from '@pnpm/semver.peer-range'

export interface ProjectToValidate {
  rootDir: string
  manifest: Pick<ProjectManifest, 'name' | 'peerDependencies'>
}

export function validatePeerDependencies (project: ProjectToValidate): void {
  const { name, peerDependencies } = project.manifest
  const projectId = name ?? project.rootDir
  for (const depName in peerDependencies) {
    const version = peerDependencies[depName]
    if (!isValidPeerRange(version)) {
      throw new PnpmError(
        'INVALID_PEER_DEPENDENCY_SPECIFICATION',
        `The peerDependencies field named '${depName}' of package '${projectId}' has an invalid value: '${version}'`,
        {
          hint: 'The values in peerDependencies should be either a valid semver range, a `workspace:` spec, or a `catalog:` spec',
        }
      )
    }
  }
}
