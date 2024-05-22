import path from 'path'
import fs from 'fs'
import { spawnSync } from 'child_process'
import { logger } from '@pnpm/logger'
import { PnpmError } from '@pnpm/error'
import camelcaseKeys, { type CamelCaseKeys } from 'camelcase-keys'
import _camelcase, { type Options as CamelcaseOptions } from 'camelcase'
import semverValid from 'semver/functions/valid'
import type { CamelCase, Entries, ValueOf } from 'type-fest'
import { pkgManagerFieldValid, type OptionsRaw } from './options'
import { toUpper } from 'ramda'
import { applyPrompt } from './apply-prompt.js'
import type { ManifestDefaults } from './init.js'

function camelcase<T extends string> (input: T, options?: CamelcaseOptions): CamelCase<T> {
  return _camelcase(input, options) as CamelCase<T>
}

export interface Person {
  name?: string
  email?: string
  url?: string
  web?: string
  mail?: string
}

export function personToString (person: Person): string {
  const name = person.name ?? ''
  const u = person.url ?? person.web
  const url = u ? ` (${u})` : ''
  const e = person.email ?? person.mail
  const email = e ? ` <${e}>` : ''
  return name + email + url
}

type CamelInitOptions = CamelCaseKeys<OptionsRaw>
type CamelInitOptionsFiltered = Omit<CamelInitOptions, 'initModule'>
export function workWithInitModule (localConfig: Record<string, string> & CamelInitOptions): CamelInitOptionsFiltered {
  const { initModule, ...restConfig } = localConfig
  if (initModule) {
    const filePath = path.resolve(initModule)
    const isFileExist = fs.existsSync(filePath)
    if (['.js', '.cjs'].includes(path.extname(filePath)) && isFileExist) {
      spawnSync('node', [filePath], {
        stdio: 'inherit',
      })
    }
  }
  return restConfig
}

type LocalConfigKeys = Array<(keyof CamelInitOptionsFiltered & string)>
type UnPrefixed<T extends string, Prefix extends string> = T extends `${Prefix}${infer P}` ? P : never

type PureKey = UnPrefixed<(LocalConfigKeys[number] & `init${string}`), 'init'>
type PureCamelKey = CamelCase<PureKey>
/** Re-assert the type of the value for the given key once the key has been filtered down */
type OptType<K extends PureCamelKey> = Required<CamelInitOptionsFiltered>[`init${Capitalize<K>}`]

// if key is prefixed, ASSERT that it is prefixed and filter out the rest
const isPrefix = <KeyKind extends string, Prefix extends string, Filter = `${Prefix}${string}`>(
  key: KeyKind, prefix: Prefix
): key is Extract<KeyKind, Filter> => key.startsWith(prefix)
const minusPrefix = <KeyKind extends string, Prefix extends string>(
  key: KeyKind, prefix: Prefix
): UnPrefixed<KeyKind, Prefix> => String.prototype.slice.call(key, prefix.length) as UnPrefixed<KeyKind, Prefix>

type PurePackageKeys = Exclude<CamelCase<PureKey>,
`author${string}` |
`bugs${string}` |
'ask' |
'scriptTest'
>
export type PkgJsonType = {
  [K in PurePackageKeys]?: CamelInitOptionsFiltered[`init${Capitalize<K>}`]
} & {
  author?: string
  bugs?: string | { url?: string, email?: string }
  publishConfig?: Record<string, unknown>
  contributors?: string[]
  funding?: string | string[]
  scripts?: Record<string, string>
}

export const getMetaOptions = (opts: {
  loglevel?: 'silent' | 'error' | 'warn' | 'info' | 'debug'
  force?: true
  'workspace-update'?: string
  'init-ask'?: boolean | 'extended' | 'npm' | 'none'
}): { silent: boolean, force: boolean, initAsk: boolean | 'extended' | 'npm' | 'none', workspaceUpdate: boolean } => ({
  silent: ['silent', 'error'].includes(opts.loglevel ?? ''),
  force: !!opts.force,
  initAsk: opts['init-ask'] ?? false,
  workspaceUpdate: !!opts['workspace-update'],
})

export const packageNameValidator = (value: string): string | true => {
  const maxLength = 214
  const isScoped = value.startsWith('@')
  const hasInvalidCharacters = /[^a-zA-Z0-9-_]/.test(value)
  const hasUppercaseLetters = /[A-Z]/.test(value)

  if (value.length > maxLength) {
    return `The name must be less than or equal to ${maxLength} characters.`
  }

  if (isScoped && !value.includes('/')) {
    return 'Scoped packages must include a scoped name (e.g., @scope/package-name).'
  }

  if (!isScoped && (value.startsWith('.') || value.startsWith('_'))) {
    return 'Only scoped packages can begin with a dot or an underscore.'
  }

  if (hasInvalidCharacters) {
    return 'The name contains non-URL-safe characters.'
  }

  if (hasUppercaseLetters) {
    return 'New packages must not have uppercase letters in the name.'
  }

  return true
}

export const AskLevel = {
  none: 0,
  npm: 1,
  extended: 2,
} as const
const getAskLevel = (ask: boolean | 'extended' | 'npm' | 'none' | undefined): ValueOf<typeof AskLevel> => {
  if (ask === 'extended') return AskLevel.extended
  if (ask === 'npm' || ask === true) return AskLevel.npm
  return AskLevel.none
}

export async function workWithInitConfig (localConfig: CamelInitOptionsFiltered, manifestDefaults?: ManifestDefaults): Promise<PkgJsonType> {
  const { initAsk, loglevel, force: _force, ...lConfig } = localConfig
  const askLevel = getAskLevel(initAsk)
  const { silent, force } = getMetaOptions({ loglevel, force: _force })

  const packageJson: PkgJsonType = manifestDefaults ? Object.assign({}, manifestDefaults) : {}
  let authorInfo: {
    name?: string
    email?: string
    url?: string
  } | string = {}
  const bugs = {
    url: '',
    email: '',
  }

  for (const [lKey, lValue] of Object.entries(lConfig) as Entries<typeof lConfig>) {
    // Init keys
    if (isPrefix(lKey, 'init')) {
      const initKey = minusPrefix(lKey, 'init')
      const pureKey = camelcase(initKey)
      if (lValue === undefined || lValue === null) continue

      const urlValidator = (value: string) => {
        try {
          return new URL(value).toString()
        } catch (_error) {
          const message = `Invalid URL string for ${pureKey}: ${value}`
          if (force) !silent || logger.warn({ message, prefix: process.cwd() })
          else throw new PnpmError(`INIT_${toUpper(pureKey)}_PARSE`, message)
          return undefined
        }
      }

      // Author info
      if (isPrefix(pureKey, 'author')) {
        if (typeof authorInfo === 'string') continue
        const v = lValue as OptType<typeof pureKey>
        const authorKey = camelcase(minusPrefix(pureKey, 'author'))
        if (authorKey !== '') {
          if (authorKey === 'url') authorInfo[authorKey] = urlValidator(v)
          else authorInfo[authorKey] = v
        } else {
          authorInfo = v
        }
        continue
      }

      // Bugs info
      if (isPrefix(pureKey, 'bugs')) {
        const k = camelcase(minusPrefix(pureKey, 'bugs'))
        const v = lValue as OptType<typeof pureKey>
        if (k === 'url') {
          const validated = urlValidator(v)
          if (validated) bugs.url = validated
        } else bugs[k] = v
        continue
      }

      // Validated URLs
      if (pureKey === 'funding') {
        const v = lValue as OptType<typeof pureKey>
        if (typeof v === 'string') {
          packageJson[pureKey] = urlValidator(v)
        } else {
          const funding = v
            .map(urlValidator)
            .filter((v): v is string => v !== undefined)
          packageJson[pureKey] = funding
        }
        continue
      }
      if (pureKey === 'homepage') {
        const v = lValue as OptType<typeof pureKey>
        const validated = urlValidator(v)
        if (validated) packageJson[pureKey] = validated
        continue
      }

      // Uniquely validated keys
      if (pureKey === 'private') {
        const v = lValue as OptType<typeof pureKey>
        if (typeof v === 'boolean' && v) packageJson[pureKey] = v
        else {
          const message = `Invalid boolean flag for ${pureKey}: ${v as string}`
          if (force) !silent || logger.warn({ message, prefix: process.cwd() })
          else throw new PnpmError(`INIT_${toUpper(pureKey)}_PARSE`, message)
        }
        continue
      }
      if (pureKey === 'type') {
        const v = lValue as OptType<typeof pureKey>
        if (v === 'module' || v === 'commonjs') packageJson[pureKey] = v
        else {
          const message = `Invalid boolean flag for ${pureKey}: ${v as string}`
          if (force) !silent || logger.warn({ message, prefix: process.cwd() })
          else throw new PnpmError(`INIT_${toUpper(pureKey)}_PARSE`, message)
        }
        continue
      }
      if (pureKey === 'version') {
        const v = lValue as OptType<typeof pureKey>
        const validated = semverValid(v)
        if (validated) packageJson[pureKey] = validated
        else {
          const message = `Invalid version string for ${pureKey} flag: ${v}`
          if (force) !silent || logger.warn({ message, prefix: process.cwd() })
          else throw new PnpmError(`INIT_${toUpper(pureKey)}_PARSE`, message)
        }
        continue
      }
      if (pureKey === 'packageManager') {
        const v = lValue as OptType<typeof pureKey>
        if (pkgManagerFieldValid(v)) packageJson[pureKey] = v
        else {
          const message = `Invalid package manager (w/version) string for ${pureKey} flag: ${v as string}`
          if (force) !silent || logger.warn({ message, prefix: process.cwd() })
          else throw new PnpmError('INIT_PKG_MANAGER_PARSE', message)
        }
        continue
      }
      if (pureKey === 'name') {
        const v = lValue as OptType<typeof pureKey>
        const validated = packageNameValidator(v)
        if (validated === true) packageJson[pureKey] = v
        else {
          const message = `Invalid package name string for ${pureKey} flag: ${validated}`
          if (force) !silent || logger.warn({ message, prefix: process.cwd() })
          else throw new PnpmError('INIT_PKG_MANAGER_PARSE', message)
        }
        continue
      }

      // Structured transforms
      if (pureKey === 'publishConfig') {
        const v = lValue as OptType<typeof pureKey>
        try {
          const parsed = JSON.parse(v)
          packageJson[pureKey] = parsed
        } catch (_error) {
          const message = `Invalid JSON string for publishConfig: ${v}`
          if (force) !silent || logger.warn({ message, prefix: process.cwd() })
          else throw new PnpmError('INIT_PUBLISH_CFG_PARSE', message)
        }
        continue
      }
      if (pureKey === 'scriptTest') {
        const v = lValue as OptType<typeof pureKey>
        packageJson.scripts = { test: v }
        continue
      }

      // All remaining keys (hover over `pureKey` to see the inferred type of the remaining keys to compare)
      if (pureKey === 'contributors') {
        const v = lValue as OptType<typeof pureKey>
        if (typeof v === 'string') {
          packageJson[pureKey] = [v]
        } else {
          packageJson[pureKey] = v
        }
        continue
      }
      if (pureKey === 'keywords') {
        const v = lValue as OptType<typeof pureKey>
        if (typeof v === 'string') {
          packageJson[pureKey] = [v]
        } else {
          packageJson[pureKey] = v
        }
        continue
      }
      if (pureKey === 'license') packageJson[pureKey] = lValue as OptType<typeof pureKey>
      else if (pureKey === 'description') packageJson[pureKey] = lValue as OptType<typeof pureKey>
      else if (pureKey === 'main') packageJson[pureKey] = lValue as OptType<typeof pureKey>
      else if (pureKey === 'repository') packageJson[pureKey] = lValue as OptType<typeof pureKey>
    }
  }

  if (lConfig.scope) {
    const atSign = packageJson.name?.startsWith('@') ? '' : '@'
    if (packageJson.name) packageJson.name = `${atSign}${lConfig.scope}/${packageJson.name}`
    else packageJson.name = `${atSign}${lConfig.scope}/${path.basename(process.cwd())}`
  }

  const author = typeof authorInfo === 'string'
    ? authorInfo
    : personToString(camelcaseKeys(authorInfo))
  if (author !== '') {
    packageJson.author = author
  }
  if (bugs.email !== '' || bugs.url !== '') {
    if (bugs.email === '') packageJson.bugs = bugs.url
    else packageJson.bugs = bugs
  }

  if (askLevel > AskLevel.none)
    await applyPrompt({
      force,
      silent,
      askLevel,
      packageJson,
    })

  return packageJson
}

export async function parseRawConfig (rawConfig: Record<string, string> & OptionsRaw, manifestDefaults?: ManifestDefaults): Promise<PkgJsonType> {
  return workWithInitConfig(
    workWithInitModule(camelcaseKeys(rawConfig)),
    manifestDefaults
  )
}
