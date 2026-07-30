let gitEnv: NodeJS.ProcessEnv | undefined

/**
 * Environment for spawning git with interactive credential prompts disabled,
 * so ls-remote fails fast on private repos instead of blocking on user input.
 * The snapshot is cached because copying process.env on every git invocation
 * is measurably slow.
 */
export function getGitEnv (): NodeJS.ProcessEnv {
  if (process.env.GIT_TERMINAL_PROMPT === '0') return process.env
  if (gitEnv == null) {
    gitEnv = { ...process.env, GIT_TERMINAL_PROMPT: '0' }
  }
  return gitEnv
}
