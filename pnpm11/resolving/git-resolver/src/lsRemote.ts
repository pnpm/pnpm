import { safeExeca as execa } from 'execa'

/**
 * Runs `git ls-remote` with interactive credential prompts disabled, so it
 * fails fast on private repos instead of blocking on user input. All
 * ls-remote invocations must go through this function to keep that guarantee.
 *
 * git is spawned directly rather than through graceful-git because
 * graceful-git forwards only `cwd` to the child process, which would drop
 * the `GIT_TERMINAL_PROMPT` override. Failed runs are retried immediately,
 * matching the Rust runner's policy.
 */
export async function lsRemote (args: string[], opts: { retries: number }): Promise<{ stdout: string }> {
  let lastErr: unknown
  for (let attempt = 0; attempt <= opts.retries; attempt++) {
    try {
      const { stdout } = await execa('git', ['ls-remote', ...args], { // eslint-disable-line no-await-in-loop
        // Snapshotted per call so changes to auth/proxy env vars made by a
        // long-lived host process are picked up.
        env: { ...process.env, GIT_TERMINAL_PROMPT: '0' },
      })
      return { stdout: stdout as string }
    } catch (err: unknown) {
      lastErr = err
    }
  }
  throw lastErr
}
