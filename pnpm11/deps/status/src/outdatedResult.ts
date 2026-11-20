import { isError } from '@pnpm/error'
import type { WorkspaceState } from '@pnpm/workspace.state'

import type { CheckDepsStatusResult } from './types.js'

export function outdatedResult (issue: string | undefined, workspaceState: WorkspaceState): CheckDepsStatusResult {
  return { upToDate: false, issue, workspaceState }
}

export function errorMessageOf (err: unknown): string | undefined {
  return (isError(err) && 'message' in err) ? err.message : undefined
}
