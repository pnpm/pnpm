import { PnpmError } from '@pnpm/error'

/**
 * tokenHelper names an executable pnpm runs, so it must only come from trusted,
 * non-repo config sources (~/.npmrc and the global auth.ini) — never from a
 * workspace or project .npmrc, which could otherwise execute arbitrary commands.
 * The trusted config merges exactly those trusted sources and excludes the repo ones.
 */
export function assertTokenHelperComesFromTrustedConfig (
  authConfig: Record<string, unknown>,
  trustedConfig: Record<string, unknown>
): void {
  for (const [key, value] of Object.entries(authConfig)) {
    if (!key.endsWith('tokenHelper') && key !== 'tokenHelper') continue
    if (!(key in trustedConfig) || trustedConfig[key] !== value) {
      throw new PnpmError('TOKEN_HELPER_IN_PROJECT_CONFIG',
        'tokenHelper must not be configured in project-level .npmrc',
        { hint: `The key "${key}" was found in project config. Move it to ~/.npmrc or the global pnpm auth.ini.` })
    }
  }
}
