import { type ProjectManifest } from '@pnpm/types'
import { type ExportedManifest } from './index.js'

type Input = Pick<ProjectManifest, 'peerDependenciesMeta'>
type Omitted<Manifest extends Input> = Omit<Manifest, 'peerDependenciesMeta'>
type Output<Manifest extends Input> = Omitted<Manifest> & Pick<ExportedManifest, 'peerDependenciesMeta'>

export function transformPeerDependenciesMeta<Manifest extends Input> (manifest: Manifest): Output<Manifest> {
  if (!manifest.peerDependenciesMeta) return manifest as Omitted<Manifest>

  const inputPeerDepsMeta = manifest.peerDependenciesMeta
  const outputPeerDepsMeta: Required<ExportedManifest>['peerDependenciesMeta'] = {}
  for (const key in inputPeerDepsMeta) {
    const { optional, ...rest } = inputPeerDepsMeta[key]
    outputPeerDepsMeta[key] = {
      ...rest,
      optional: optional ?? false,
    }
  }

  return {
    ...manifest as Omitted<Manifest>,
    peerDependenciesMeta: outputPeerDepsMeta,
  }
}
