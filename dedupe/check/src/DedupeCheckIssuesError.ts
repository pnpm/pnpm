import { type DedupeCheckIssues } from '@pnpm/dedupe.types'
import { PnpmError } from '@pnpm/error'

export class DedupeCheckIssuesError extends PnpmError {
  constructor (public dedupeCheckIssues: DedupeCheckIssues) {
    super('DEDUPE_CHECK_ISSUES', 'Dedupe --check found changes to the lockfile')
  }
}
