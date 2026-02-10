import { normalizeBinObject } from '@pnpm/package-bins'
import { type ProjectManifest } from '@pnpm/types'
import { type ExportedManifest } from './index.js'

type Input = Pick<ProjectManifest, 'bin'> & Pick<ExportedManifest, 'name'>
type Output<Manifest extends Input> = Omit<Manifest, 'bin'> & Pick<ExportedManifest, 'bin'>

export function transformBin<Manifest extends Input> (manifest: Manifest): Output<Manifest> {
  if (manifest.bin == null || typeof manifest.bin === 'object') return manifest as Output<Manifest>
  const { bin, ...rest } = manifest
  return {
    ...rest,
    bin: normalizeBinObject(manifest.name, bin),
  }
}
