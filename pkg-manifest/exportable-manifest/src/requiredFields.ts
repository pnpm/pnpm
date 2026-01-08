import { PnpmError } from '@pnpm/error'
import { type ProjectManifest } from '@pnpm/types'
import { type ExportedManifest } from './index.js'

type RequiredField = 'name' | 'version'
type Input = Pick<ProjectManifest, RequiredField>
type Output = Pick<ExportedManifest, RequiredField>

export function transformRequiredFields<Manifest> (manifest: Manifest & Input): Manifest & Output {
  validateRequiredFields(manifest)
  return manifest
}

export function validateRequiredFields<Manifest> (manifest: Manifest & Input): asserts manifest is Manifest & Output {
  if (!manifest.name) throw new MissingRequiredFieldError('name')
  if (!manifest.version) throw new MissingRequiredFieldError('version')
}

export class MissingRequiredFieldError<Field extends RequiredField> extends PnpmError {
  readonly field: Field
  constructor (field: Field) {
    super('MISSING_REQUIRED_FIELD', `Missing required field ${JSON.stringify(field)}`)
    this.field = field
  }
}
