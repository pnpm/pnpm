import { gracefulGit as git } from 'graceful-git'

/**
 * Runs `git ls-remote` with interactive credential prompts disabled, so it
 * fails fast on private repos instead of blocking on user input. All
 * ls-remote invocations must go through this function to keep that guarantee.
 */
export async function lsRemote (args: string[], opts: { retries: number }): Promise<{ stdout: string }> {
  return git(['ls-remote', ...args], {
    retries: opts.retries,
    // Snapshotted per call so changes to auth/proxy env vars made by a
    // long-lived host process are picked up.
    env: { ...process.env, GIT_TERMINAL_PROMPT: '0' },
  })
}
