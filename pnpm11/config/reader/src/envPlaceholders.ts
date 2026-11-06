import { PnpmError } from '@pnpm/error'

export function hasEnvPlaceholder (value: string): boolean {
  return /\$\{[^}]+\}/.test(value)
}

/**
 * A repository-controlled file names a registry whose URL holds an environment
 * variable placeholder. pnpm does not expand it, and dropping the URL would
 * resolve the packages it serves from another registry, so the setting fails
 * instead of being ignored.
 *
 * `setting` must already be redacted.
 */
export class RegistryEnvNotExpandedError extends PnpmError {
  constructor (setting: string, filePath: string) {
    super('REGISTRY_ENV_NOT_EXPANDED',
      `The "${setting}" registry URL in "${filePath}" contains an environment variable`,
      {
        hint: 'pnpm does not expand environment variables in registry URLs that come from a file committed to the repository. ' +
          'Ignoring the URL would resolve the packages it serves from another registry. ' +
          'Write the URL literally, or move the setting to the global config.yaml.',
      })
  }
}
