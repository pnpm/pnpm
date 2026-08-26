import { type OptionsFromRootManifest } from '@pnpm/config'

export function mergeOverrides (
  rootManifestOptions: OptionsFromRootManifest,
  overrides?: Record<string, string>
): OptionsFromRootManifest {
  if (!rootManifestOptions.overrides || !overrides) return rootManifestOptions
  return {
    ...rootManifestOptions,
    overrides: {
      ...rootManifestOptions.overrides,
      ...overrides,
    },
  }
}
