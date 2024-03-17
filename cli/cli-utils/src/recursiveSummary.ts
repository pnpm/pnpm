import { PnpmError } from '@pnpm/error'

interface ActionFailure {
  status: 'failure'
  duration?: number
  prefix: string
  message: string
  error: Error
}

interface ActionPassed {
  status: 'passed'
  duration?: number
}

interface ActionQueued {
  status: 'queued'
}

interface ActionRunning {
  status: 'running'
  duration?: number
}

interface ActionSkipped {
  status: 'skipped'
}

export type Actions =
  | ActionPassed
  | ActionQueued
  | ActionRunning
  | ActionSkipped
  | ActionFailure

export type RecursiveSummary = Record<string, Actions>

class RecursiveFailError extends PnpmError {
  public readonly failures: ActionFailure[]
  public readonly passes: number

  constructor(
    command: string,
    recursiveSummary: RecursiveSummary,
    failures: ActionFailure[]
  ) {
    super(
      'RECURSIVE_FAIL',
      `"${command}" failed in ${failures.length} packages`
    )

    this.failures = failures
    this.passes = Object.values(recursiveSummary).filter(
      ({ status }) => status === 'passed'
    ).length
  }
}

export function throwOnCommandFail(
  command: string,
  recursiveSummary: RecursiveSummary
) {
  const failures = Object.values(recursiveSummary).filter(
    ({ status }: Actions) => status === 'failure'
  ) as ActionFailure[]
  if (failures.length > 0) {
    throw new RecursiveFailError(command, recursiveSummary, failures)
  }
}
