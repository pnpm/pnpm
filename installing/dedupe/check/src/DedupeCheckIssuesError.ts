import type { DedupeCheckIssues } from '@pnpm/dedupe.types'
import { PnpmError } from '@pnpm/error'

export class DedupeCheckIssuesError extends PnpmError {
  public dedupeCheckIssues: DedupeCheckIssues

  constructor (dedupeCheckIssues: DedupeCheckIssues) {
    super('DEDUPE_CHECK_ISSUES', 'Dedupe --check found changes to the lockfile')
    this.dedupeCheckIssues = dedupeCheckIssues
  }
}
