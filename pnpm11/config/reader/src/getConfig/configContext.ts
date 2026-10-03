import type { Config, ConfigContext } from '../Config.js'

/**
 * The reader's own bookkeeping, which shares one object with the settings but
 * is not settable by anyone.
 *
 * A manifest naming one of these would not choose a setting: it would
 * overwrite what the reader worked out, with a value of the wrong type.
 */
export const CONFIG_CONTEXT_KEYS = [
  'hooks',
  'finders',
  'allProjects',
  'selectedProjectsGraph',
  'allProjectsGraph',
  'prodAllProjectsGraph',
  'prodOnlySelectedProjectDirs',
  'rootProjectManifest',
  'rootProjectManifestDir',
  'enginePinManifest',
  'nodeVersionFromEnginesRuntime',
  'cliOptions',
  'rawCliConfig',
  'explicitlySetKeys',
  'packageManager',
  'wantedPackageManager',
] as const satisfies ReadonlyArray<keyof ConfigContext>

type ProofConfigContextKeysIsExhaustive =
  (_: Record<typeof CONFIG_CONTEXT_KEYS[number], unknown>) => Record<keyof ConfigContext, unknown>

const _proofConfigContextKeysIsExhaustive: ProofConfigContextKeysIsExhaustive = (record) => record

export const CONFIG_CONTEXT_KEY_SET: ReadonlySet<string> = new Set(CONFIG_CONTEXT_KEYS)

/** Separates the reader's bookkeeping from the settings it resolved. */
export function splitConfigAndContext (pnpmConfig: Config & ConfigContext): { config: Config, context: ConfigContext } {
  const context = Object.fromEntries(CONFIG_CONTEXT_KEYS.map((key) => [key, pnpmConfig[key]])) as unknown as ConfigContext
  const config: Partial<Config & ConfigContext> = { ...pnpmConfig }
  for (const key of CONFIG_CONTEXT_KEYS) {
    delete config[key]
  }
  return { config: config as Config, context }
}
