import util from 'node:util'

export function errorMessage (err: unknown): string {
  return util.types.isNativeError(err) ? err.message : String(err)
}
