import { isAcceptablePeerSpec } from '@pnpm/deps.peer-range'
import { PnpmError } from '@pnpm/error'
import type { ProjectManifest } from '@pnpm/types'

export interface ProjectToValidate {
  rootDir: string
  manifest: Pick<ProjectManifest, 'name' | 'peerDependencies'>
}

export function validatePeerDependencies (project: ProjectToValidate): void {
  const { name, peerDependencies } = project.manifest
  const projectId = name ?? project.rootDir
  for (const depName in peerDependencies) {
    const version = peerDependencies[depName]
    if (!isAcceptablePeerSpec(version)) {
      throw new PnpmError(
        'INVALID_PEER_DEPENDENCY_SPECIFICATION',
        `The peerDependencies field named '${depName}' of package '${projectId}' has an invalid value: '${version}'`,
        {
          hint: 'The values in peerDependencies should be a valid semver range, a `workspace:`/`catalog:` spec, or a dependency specifier such as a named-registry (`<registry>:<version>`), `npm:`, `file:`, or git/URL spec',
        }
      )
    }
  }
}
