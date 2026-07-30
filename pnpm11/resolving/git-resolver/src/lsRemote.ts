import { gracefulGit as git } from 'graceful-git'

/**
 * Runs `git ls-remote` with interactive credential prompts disabled, so it
 * fails fast on private repos instead of blocking on user input. All
 * ls-remote invocations must go through this function to keep that guarantee.
 */
export async function lsRemote (args: string[], opts: { retries: number }): Promise<{ stdout: string }> {
  return git(['ls-remote', ...args], { retries: opts.retries, env: getGitEnv() })
}

let gitEnv: NodeJS.ProcessEnv | undefined

// The snapshot is cached because copying process.env on every git invocation
// is measurably slow.
function getGitEnv (): NodeJS.ProcessEnv {
  if (process.env.GIT_TERMINAL_PROMPT === '0') return process.env
  if (gitEnv == null) {
    gitEnv = { ...process.env, GIT_TERMINAL_PROMPT: '0' }
  }
  return gitEnv
}
