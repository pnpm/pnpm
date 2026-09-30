import { isError } from '@pnpm/error'
import { globalInfo } from '@pnpm/logger'

/**
 * Cancelling a prompt with Ctrl-c is how the user declines to update, not an
 * error: report it and leave with a success status.
 */
export async function runUpdatePrompt<Answer> (prompt: () => Promise<Answer>): Promise<Answer> {
  try {
    return await prompt()
  } catch (err: unknown) {
    if (isError(err) && err.name === 'ExitPromptError') {
      globalInfo('Update canceled')
      // eslint-disable-next-line n/no-process-exit -- a declined prompt ends the command with a success status, and there is no result to return to the caller
      process.exit(0)
    }
    throw err
  }
}

export function describeUpToDate (latest: boolean | undefined): string {
  return latest
    ? 'All of your dependencies are already up to date'
    : 'All of your dependencies are already up to date inside the specified ranges. Use the --latest option to update the ranges in package.json'
}
