interface ActionFailure {
  prefix: string,
  message: string,
  error: Error,
}

interface RecursiveSummary {
  fails: ActionFailure[],
  passes: number,
}

export default RecursiveSummary

export function throwOnCommandFail (command: string, recursiveSummary: RecursiveSummary) {
  if (!recursiveSummary.fails.length) return

  const err = new Error(`"${command}" failed in ${recursiveSummary.fails.length} packages`)
  // tslint:disable:no-string-literal
  err['fails'] = recursiveSummary.fails
  err['passes'] = recursiveSummary.passes
  err['code'] = 'ERR_PNPM_RECURSIVE_FAIL'
  // tslint:enable:no-string-literal
  throw err
}
