import { PnpmError } from '@pnpm/error'
import { type Dependencies } from '@pnpm/types'
import { validRange } from 'semver'

export function validatePeerDependencies (peerDependencies: Dependencies | undefined): void {
  for (const name in peerDependencies) {
    const spec = peerDependencies[name]
    if (!isValidPeerSpec(spec)) {
      throw new PnpmError('INVALID_PEER_DEPENDENCY_SPECIFICATION', `The peer dependency named ${name} has unacceptable specification: ${spec}`)
    }
  }
}

function isValidPeerSpec (spec: string): boolean {
  if (isValidPeerVersion(spec)) return true

  const parseResult = parseAliasedSpec(spec)
  return parseResult ? isValidPeerVersion(parseResult.version) : false
}

function isValidPeerVersion (version: string): boolean {
  return typeof validRange(version) === 'string' || version.startsWith('npm:') || version.startsWith('workspace:') || version.startsWith('catalog:')
}

function parseAliasedSpec (spec: string): { version: string } | undefined {
  const splitIndex = spec.indexOf('@', 1)
  if (splitIndex === -1) return undefined
  const version = spec.slice(splitIndex + '@'.length)
  return { version }
}
