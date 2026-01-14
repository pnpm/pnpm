import { PnpmError } from '@pnpm/error'
import { type ProjectManifest } from '@pnpm/types'
import { type ExportedManifest } from './index.js'

type EnginesField = 'engines' | 'devEngines'
type Input = Pick<ProjectManifest, EnginesField>
type Output<Manifest extends Input> = Omit<Manifest, EnginesField> & Pick<ExportedManifest, EnginesField>

export function transformEngines<Manifest extends Input> (manifest: Manifest): Output<Manifest> {
  if (!manifest.engines?.runtime) return manifest as Output<Manifest>

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
  } as Output<Manifest>
}

export class DevEnginesRuntimeConflictError extends PnpmError {
  constructor () {
    super('DEV_ENGINES_RUNTIME_CONFLICT', '.devEngines.runtime and .engines.runtime were both defined')
  }
}
