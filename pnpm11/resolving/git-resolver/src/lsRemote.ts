import { PnpmError, redactAndSanitizeMultiline } from '@pnpm/error'
import { safeExeca as execa } from 'execa'

/**
 * Runs `git ls-remote` with interactive credential prompts disabled, so it
 * fails fast on private repos instead of blocking on user input. All
 * ls-remote invocations must go through this function to keep that guarantee.
 *
 * Failed runs are retried immediately, matching the Rust runner's policy.
 * A run that fails every attempt throws `ERR_PNPM_GIT_LS_REMOTE_FAILED`,
 * which the git resolver restates with the dependency it was resolving.
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
  throw lsRemoteError(lastErr)
}

/**
 * git's stderr is untrusted: the repository URL it echoes back can carry
 * `user:pass@` credentials, so it goes through
 * {@link redactAndSanitizeMultiline} rather than being restated verbatim.
 */
function lsRemoteError (err: unknown): PnpmError {
  return new PnpmError('GIT_LS_REMOTE_FAILED', `git ls-remote failed: ${redactAndSanitizeMultiline(lsRemoteFailureDetail(err))}`)
}

function lsRemoteFailureDetail (err: unknown): string {
  if ((err as { code?: string }).code === 'ENOENT') {
    return '`git` executable not found on PATH. Install git to resolve git-hosted packages.'
  }
  const stderr = (err as { stderr?: string }).stderr?.trim()
  if (stderr != null && stderr !== '') return stderr
  return (err as { message?: string }).message ?? String(err)
}
