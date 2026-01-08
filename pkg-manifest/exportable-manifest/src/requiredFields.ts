import { PnpmError } from '@pnpm/error'
import { type DependencyManifest, type ProjectManifest } from '@pnpm/types'

type RequiredField = 'name' | 'version'

export function validateRequiredFields (manifest: ProjectManifest): asserts manifest is DependencyManifest {
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
