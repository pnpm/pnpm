import { isError, PnpmError } from '@pnpm/error'

export function workflowError (operation: 'PARSE' | 'READ' | 'WRITE', filePath: string, cause: unknown): PnpmError {
  const detail = isError(cause) ? cause.message : String(cause)
  return new PnpmError(`GITHUB_ACTIONS_WORKFLOW_${operation}`, `Failed to ${operation.toLowerCase()} GitHub Actions workflow ${filePath}: ${detail}`, { cause })
}

export function isErrorCode (err: unknown, code: string): err is NodeJS.ErrnoException {
  return isError(err) && 'code' in err && err.code === code
}
