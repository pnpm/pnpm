import { PnpmError } from '@pnpm/error'
import { type Dependencies } from '@pnpm/types'
import { validRange } from 'semver'

export function validatePeerDependencies (peerDependencies: Dependencies | undefined): void {
  for (const name in peerDependencies) {
    const version = peerDependencies[name]
    if (!isValidPeerVersion(version)) {
      throw new PnpmError('INVALID_PEER_DEPENDENCY_SPECIFICATION', `The peer dependency named ${name} has unacceptable specification: ${version}`)
    }
  }
}

function isValidPeerVersion (version: string): boolean {
  return typeof validRange(version) === 'string' || version.startsWith('workspace:') || version.startsWith('catalog:')
}
