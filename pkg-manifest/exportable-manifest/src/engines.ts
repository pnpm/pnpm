import { PnpmError } from '@pnpm/error'
import { type ProjectManifest } from '@pnpm/types'
import { type ExportedManifest } from './index.js'

type Input = Pick<ProjectManifest, 'engines' | 'devEngines'>
type Output = Pick<ExportedManifest, 'engines' | 'devEngines'>

export function transformEngines<Manifest> (manifest: Manifest & Input): Manifest & Output {
  if (!manifest.engines?.runtime) return manifest as Manifest & Output

  if (manifest.engines.runtime && manifest.devEngines?.runtime) {
    throw new DevEnginesRuntimeConflictError()
  }

  const {
    engines: { runtime, ...engines },
    ...rest
  } = manifest

  return {
    ...rest,
    engines,
    devEngines: {
      ...rest.devEngines,
      runtime,
    },
  } as Manifest & Output
}

export class DevEnginesRuntimeConflictError extends PnpmError {
  constructor () {
    super('DEV_ENGINES_RUNTIME_CONFLICT', '.devEngines.runtime and .engines.runtime were both defined')
  }
}
