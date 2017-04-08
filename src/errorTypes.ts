export type PnpmErrorCode = 'UNEXPECTED_STORE'
  | 'STORE_BREAKING_CHANGE'
  | 'MODULES_BREAKING_CHANGE'
  | 'SHRINKWRAP_BREAKING_CHANGE'
  | 'MODIFIED_DEPENDENCY'
  | 'OUTDATED_NODE_MODULES'
  | 'OUTDATED_SHRINKWRAP_FILE'
  | 'NO_OFFLINE_META'
  | 'NO_OFFLINE_TARBALL'

export class PnpmError extends Error {
  constructor (code: PnpmErrorCode, message: string) {
    super(message)
    this.code = code
  }
  code: PnpmErrorCode
}
