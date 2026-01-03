import { type ProjectManifest } from "@pnpm/types"
import { PnpmError } from "@pnpm/error"

// TODO: transform the `engines` field

export interface PublishableManifest extends ProjectManifest {
  name: string
  version: string
  bundleDependencies?: never // for the sake of simplicity, pnpm refuses to transform this field for now
  bundledDependencies?: never // for the sake of simplicity, pnpm refuses to transform this field for now
}

export function assertPublishableManifest (manifest: ProjectManifest): asserts manifest is PublishableManifest {
  if (!manifest.name) throw new MissingRequiredFieldError('name')
  if (!manifest.version) throw new MissingRequiredFieldError('version')
  if (manifest.bundleDependencies) throw new UnsupportedBundleDepsError('bundleDependencies')
  if (manifest.bundledDependencies) throw new UnsupportedBundleDepsError('bundledDependencies')
}

export class MissingRequiredFieldError<Field extends 'name' | 'version'> extends PnpmError {
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
