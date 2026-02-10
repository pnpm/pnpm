import { type PackageJSON as ExportedManifest } from '@npm/types'
import { type ProjectManifest } from '@pnpm/types'
import { pipe } from 'ramda'
import { transformBin } from './bin.js'
import { transformEngines } from './engines.js'
import { transformRequiredFields } from './requiredFields.js'
import { transformPeerDependenciesMeta } from './peerDependenciesMeta.js'

export { type ExportedManifest }

export type Transform = (manifest: ProjectManifest) => ExportedManifest
export const transform: Transform = pipe(
  transformRequiredFields,
  transformBin,
  transformEngines,
  transformPeerDependenciesMeta
)
