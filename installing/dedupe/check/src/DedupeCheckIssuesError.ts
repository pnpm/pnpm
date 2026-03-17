import { PnpmError } from '@pnpm/error'
import type { DedupeCheckIssues } from '@pnpm/installing.dedupe.types'

export class DedupeCheckIssuesError extends PnpmError {
  public dedupeCheckIssues: DedupeCheckIssues

  constructor (dedupeCheckIssues: DedupeCheckIssues) {
    super('DEDUPE_CHECK_ISSUES', 'Dedupe --check found changes to the lockfile')
    this.dedupeCheckIssues = dedupeCheckIssues
  }
}
