import { PnpmError } from '@pnpm/error'
import { type ProjectManifest } from '@pnpm/types'
import { type ExportedManifest } from './index.js'

type RequiredField = 'name' | 'version'
type Input = Pick<ProjectManifest, RequiredField>
type Output<Manifest extends Input> = Omit<Manifest, RequiredField> & Pick<ExportedManifest, RequiredField>

export function transformRequiredFields<Manifest extends Input> (manifest: Manifest): Output<Manifest> {
  if (!manifest.name) throw new MissingRequiredFieldError('name')
  if (!manifest.version) throw new MissingRequiredFieldError('version')
  return manifest as Output<Manifest>
}

export class MissingRequiredFieldError<Field extends RequiredField> extends PnpmError {
  readonly field: Field
  constructor (field: Field) {
    super('MISSING_REQUIRED_FIELD', `Missing required field ${JSON.stringify(field)}`)
    this.field = field
  }
}
