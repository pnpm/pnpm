import { readFile } from 'node:fs/promises'
import path from 'node:path'

import { PnpmError } from '@pnpm/error'

/** Fields pack-app reads from `pnpm.app` in package.json. */
export interface ProjectAppConfig {
  entry?: string
  targets?: string[]
  runtime?: string
  outputDir?: string
  outputName?: string
}

export interface ReadProjectAppConfigResult {
  name?: string
  app?: ProjectAppConfig
}

// A narrow reader just for this command. Using readProjectManifest from
// @pnpm/cli.utils would pull in the installable/engine checks, which are
// irrelevant here: pack-app doesn't need the current project to be installable
// under the running Node, just to have a package.json with optional settings.
export async function readProjectAppConfig (dir: string): Promise<ReadProjectAppConfigResult> {
  let raw: string
  try {
    raw = await readFile(path.join(dir, 'package.json'), 'utf8')
  } catch {
    return {}
  }
  let manifest: unknown
  try {
    manifest = JSON.parse(raw)
  } catch (err) {
    throw new PnpmError('PACK_APP_INVALID_PACKAGE_JSON',
      `Failed to parse ${path.join(dir, 'package.json')}: ${(err as Error).message}`)
  }
  if (!isObject(manifest)) return {}

  const name = typeof manifest.name === 'string' && manifest.name !== '' ? manifest.name : undefined
  const pnpmField = isObject(manifest.pnpm) ? manifest.pnpm : undefined
  const appField = pnpmField && isObject(pnpmField.app) ? pnpmField.app : undefined
  if (!appField) return { name }
  return { name, app: validateAppConfig(appField) }
}

interface AppConfigFieldRule {
  isValid: (value: unknown) => boolean
  expected: string
}

const APP_CONFIG_FIELD_RULES: Record<keyof ProjectAppConfig, AppConfigFieldRule> = {
  entry: { isValid: isString, expected: 'a string' },
  targets: { isValid: isStringArray, expected: 'an array of strings' },
  runtime: { isValid: isString, expected: 'a string' },
  outputDir: { isValid: isString, expected: 'a string' },
  outputName: { isValid: isString, expected: 'a string' },
}

function validateAppConfig (raw: Record<string, unknown>): ProjectAppConfig {
  const known = new Set(Object.keys(APP_CONFIG_FIELD_RULES))
  for (const key of Object.keys(raw)) {
    if (!known.has(key)) {
      throw new PnpmError('PACK_APP_INVALID_CONFIG',
        `Unknown "pnpm.app.${key}" setting in package.json. Allowed keys: ${Array.from(known).join(', ')}.`)
    }
  }
  const config: Record<string, unknown> = {}
  for (const [key, rule] of Object.entries(APP_CONFIG_FIELD_RULES)) {
    const value = raw[key]
    if (value == null) continue
    if (!rule.isValid(value)) {
      throw new PnpmError('PACK_APP_INVALID_CONFIG', `"pnpm.app.${key}" must be ${rule.expected}.`)
    }
    config[key] = value
  }
  return config as ProjectAppConfig
}

function isString (value: unknown): value is string {
  return typeof value === 'string'
}

function isStringArray (value: unknown): value is string[] {
  return Array.isArray(value) && value.every(isString)
}

export function deriveOutputNameFromPackage (project: ReadProjectAppConfigResult, dir: string): string {
  if (!project.name) {
    throw new PnpmError('PACK_APP_NO_OUTPUT_NAME',
      `Could not determine the output name: package.json in ${dir} has no "name" field.`,
      { hint: 'Pass --output-name <name> or set "pnpm.app.outputName" in package.json.' }
    )
  }
  // Strip @scope/ prefix from scoped packages so the binary name is a plain
  // filename instead of "scope/name". The second validateOutputName() pass
  // downstream rejects any leftover path separators.
  return project.name.replace(/^@[^/]+\//, '')
}

function isObject (value: unknown): value is Record<string, unknown> {
  return value != null && typeof value === 'object' && !Array.isArray(value)
}
