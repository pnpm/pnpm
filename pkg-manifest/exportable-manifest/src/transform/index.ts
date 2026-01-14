import { type PackageJSON as ExportedManifest } from '@npm/types'
import { type ProjectManifest } from '@pnpm/types'
import { pipe } from 'ramda'
import { transformBin } from './bin.js'
import { transformEngines } from './engines.js'
import { transformRequiredFields } from './requiredFields.js'
import { transformPeerDependenciesMeta } from './peerDependenciesMeta.js'

export { type ExportedManifest }

// TODO: change the return type to ExportedManifest
export type Transform = (manifest: ProjectManifest) => ProjectManifest
export const transform: Transform = pipe(
  transformRequiredFields,
  transformBin,
  transformEngines,
  transformPeerDependenciesMeta
)
