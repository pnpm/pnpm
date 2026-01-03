import { type ProjectManifest } from '@pnpm/types'
import { PnpmError } from '@pnpm/error'
import { omit } from 'ramda'

// TODO: transform `bin`

type RequiredField = 'name' | 'version'
type BundleDependencies = 'bundleDependencies' | 'bundledDependencies'
type EngineField = 'engines'

type OnlyRequiredFields = Required<Pick<ProjectManifest, RequiredField>>

type OmittedField = BundleDependencies | EngineField
type ManifestWithOmissions = Omit<ProjectManifest, OmittedField>

export type PublishableManifest =
& ManifestWithOmissions
& OnlyRequiredFields
& Partial<Record<BundleDependencies, never>>
& Partial<Record<EngineField, Record<string, string>>>

const omitRuntime = omit(['runtime'])

export function publishableManifest (manifest: ProjectManifest): PublishableManifest {
  if (!manifest.name) throw new MissingRequiredFieldError('name')
  if (!manifest.version) throw new MissingRequiredFieldError('version')
  if (manifest.bundleDependencies) throw new UnsupportedBundleDepsError('bundleDependencies')
  if (manifest.bundledDependencies) throw new UnsupportedBundleDepsError('bundledDependencies')

  const { engines, ...rest } = manifest

  const publishableEngines: Record<string, string> | undefined = engines ? omitRuntime(engines) : undefined
  return {
    ...rest as OnlyRequiredFields,
    engines: publishableEngines,
  }
}

export class MissingRequiredFieldError<Field extends RequiredField> extends PnpmError {
  readonly field: Field
  constructor (field: Field) {
    super('PUBLISH_MISSING_REQUIRED_FIELD', `Missing required field: ${field}`)
    this.field = field
  }
}

export class UnsupportedBundleDepsError<Field extends 'bundleDependencies' | 'bundledDependencies'> extends PnpmError {
  readonly field: Field
  constructor (field: Field) {
    super('PUBLISH_BUNDLED_DEPENDENCIES_IS_UNSUPPORTED', `The field ${field} is not supported`, {
      hint: 'For the sake of simplicity, pnpm refuses to transform this field for now',
    })
    this.field = field
  }
}
