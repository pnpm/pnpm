import { normalizeBinObject } from '@pnpm/package-bins'
import { type ProjectManifest } from '@pnpm/types'
import { type ExportedManifest } from './index.js'

type Input = Pick<ProjectManifest, 'bin'> & Pick<ExportedManifest, 'name'>
type Output = Pick<ExportedManifest, 'bin' | 'name'>

export function transformBin<Manifest> (manifest: Manifest & Input): Manifest & Output {
  if (manifest.bin == null || typeof manifest.bin === 'object') return manifest as Manifest & Output
  const { bin, ...rest } = manifest
  return {
    ...rest,
    bin: normalizeBinObject(manifest.name, bin),
  } as Manifest & Output
}
