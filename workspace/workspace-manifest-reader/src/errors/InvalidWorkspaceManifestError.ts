import { PnpmError } from '@pnpm/error'

export class InvalidWorkspaceManifestError extends PnpmError {
  constructor (message: string) {
    super('INVALID_WORKSPACE_CONFIGURATION', message)
  }
}
