import kebabCase from 'lodash.kebabcase'
import npmTypes from '@pnpm/npm-conf/lib/types'
import { types } from '@pnpm/config'
import { type ConfigCommandOptions } from './ConfigCommandOptions'
import { isStrictlyKebabCase } from './isStrictlyKebabCase'
import { getObjectValueByPropertyPath, parsePropertyPath } from '@pnpm/object.property-path'

const isRcSetting = (kebabKey: string): boolean =>
  kebabKey.startsWith('@') || kebabKey.startsWith('//') || kebabKey in npmTypes || kebabKey in types

export type Options = Pick<ConfigCommandOptions, 'rawConfig' | 'workspaceManifest'>

/**
 * Convert a kebab-case key or a property path into an iterator of segments.
 *
 * If {@link keyOrPropertyPath} is strictly a kebab-case key, yield it exactly once.
 *
 * Otherwise, use {@link parsePropertyPath} to parse it.
 */
function * parseKeyOrPropertyPath (keyOrPropertyPath: string): Generator<string | number, void, void> {
  if (isStrictlyKebabCase(keyOrPropertyPath)) {
    yield keyOrPropertyPath // we don't parse kebab-case keys as property paths because it's not a valid JS syntax
  } else {
    yield * parsePropertyPath(keyOrPropertyPath)
  }
}

/**
 * Access a field of a combine config object.
 *
 * Since the settings from the workspace manifest overrides the settings from rc file,
 * this function would try accessing the workspace manifest first.
 *
 * Since rc config is only supposed to hold options, not workspace specific configurations,
 * this function would only fallbacks the rc config if the {@link key} is an option field.
 *
 * @param opts Options.
 * @param opts.rawConfig Raw config from rc files.
 * @param opts.workspaceManifest Raw config from the workspace manifest file.
 * @param key Field to access.
 * @returns Config value which corresponds to {@link key}.
 */
function getConfigByKey (opts: Options, key: string): unknown {
  const workspaceValue = (opts.workspaceManifest as Record<string | number, unknown> | undefined)?.[key]
  if (workspaceValue !== undefined) return workspaceValue // return if not undefined, including null

  const kebabKey = kebabCase(key)
  if (isRcSetting(kebabKey)) return opts.rawConfig[kebabKey]

  return undefined
}

/**
 * Get kebab-case key or a property path from a combined config object.
 *
 * Since the settings from the workspace manifest overrides the settings from rc file,
 * this function would try accessing the workspace manifest first.
 *
 * Since rc config is only supposed to hold options, not workspace specific configurations,
 * this function would only fallbacks the rc config if the kebab-case key or the top-level key
 * of the property path is an option field.
 *
 * @param opts Options.
 * @param opts.rawConfig Raw config from rc files.
 * @param opts.workspaceManifest Raw config from the workspace manifest file.
 * @param keyOrPropertyPath Either a kebab-case key or a property path.
 * @returns Config value.
 */
export function getRawSetting (opts: Options, keyOrPropertyPath: string): unknown {
  const [topLevelKey, ...suffix] = parseKeyOrPropertyPath(keyOrPropertyPath)
  const topLevelObject = getConfigByKey(opts, String(topLevelKey))
  return getObjectValueByPropertyPath(topLevelObject, suffix)
}

/**
 * List all settings from combined config object.
 *
 * Option fields are kebab-case, other fields are camelCase.
 *
 * Prioritize settings from the workspace manifest file.
 *
 * For the rc files, only setting whose field is an option fields that hasn't been
 * overridden by the workspace manifest file would be listed.
 *
 * @param opts Options.
 * @param opts.rawConfig Raw config from rc files.
 * @param opts.workspaceManifest Raw config from the workspace manifest file.
 * @returns The combined settings.
 */
export function listRawSettings (opts: Options): Record<string, unknown> {
  const settings: Record<string, unknown> = {}
  for (const key in opts.workspaceManifest) {
    const kebabKey = kebabCase(key)
    const targetKey = isRcSetting(kebabKey) ? kebabKey : key
    settings[targetKey] = opts.workspaceManifest[key as keyof typeof opts.workspaceManifest]
  }
  for (const key in opts.rawConfig) {
    if (key in settings) continue
    if (!isRcSetting(key)) continue
    settings[key] = opts.rawConfig[key]
  }
  return settings
}
