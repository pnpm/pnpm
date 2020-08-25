import PnpmError from '@pnpm/error'

interface ActionFailure {
  prefix: string
  message: string
  error: Error
}

export interface RecursiveSummary {
  fails: ActionFailure[]
  passes: number
}

class RecursiveFailError extends PnpmError {
  public readonly fails: ActionFailure[]
  public readonly passes: number

  constructor (command: string, recursiveSummary: RecursiveSummary) {
    super('RECURSIVE_FAIL', `"${command}" failed in ${recursiveSummary.fails.length} packages`)

    this.fails = recursiveSummary.fails
    this.passes = recursiveSummary.passes
  }
}

export function throwOnCommandFail (command: string, recursiveSummary: RecursiveSummary) {
  if (recursiveSummary.fails.length) {
    throw new RecursiveFailError(command, recursiveSummary)
  }
}
