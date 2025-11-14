import { DEFAULT_JSR_REGISTRY, DEFAULT_NPM_REGISTRY } from '@pnpm/constants'

export type RegistryKey = 'registry' | `@${string}:registry`

export type RawConfig = Partial<Record<RegistryKey, string>> & Partial<Record<string, unknown>>

export interface ConfigSource {
  path?: string
  data?: RawConfig
}

export const DEFAULT_REGISTRIES = {
  registry: DEFAULT_NPM_REGISTRY,
  '@jsr:registry': DEFAULT_JSR_REGISTRY,
} as const satisfies RawConfig

export interface Conf {
  add: (this: Conf, config: typeof DEFAULT_REGISTRIES, source: 'pnpm-builtin') => void
  sources: Record<string, ConfigSource>
}

export interface AddBuiltInRegistryOptions {
  warn: (message: string) => void
}

const SOURCE_NAMES_TO_SKIP = [
  'builtin',
  'pnpm-builtin',
]

export function addBuiltInRegistry (conf: Conf, opts: AddBuiltInRegistryOptions): void {
  const warnings = new Set<string>() // We use a set to avoid printing duplicated warnings (as there are synonymous sources under different names)
  for (const sourceName in conf.sources) {
    if (SOURCE_NAMES_TO_SKIP.includes(sourceName)) continue
    const source = conf.sources[sourceName]
    if (!source.data) continue
    const { registry: npm, '@jsr:registry': jsr } = source.data
    if (npm != null && npm !== DEFAULT_NPM_REGISTRY && jsr == null) {
      const sourceDescriptor = source.path
        ? `Config at ${source.path}`
        : `Config source ${sourceName}`
      warnings.add(`${sourceDescriptor} has overridden the 'registry' key without overriding the '@jsr:registry' key, it could leave a security vulnerability`)
    }
  }
  for (const warning of warnings) {
    opts.warn(warning)
  }
  conf.add(DEFAULT_REGISTRIES, 'pnpm-builtin')
}
