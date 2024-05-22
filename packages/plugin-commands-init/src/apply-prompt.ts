import { prompt } from 'enquirer'
import { type PkgJsonType, AskLevel, packageNameValidator } from './utils.js'
import type { ValueOf } from 'type-fest'
import semverValid from 'semver/functions/valid'
import { globalInfo } from '@pnpm/logger'

type _PromptOptions = Extract<Parameters<typeof prompt>[0], {
  name: string | (() => string)
  type: string | (() => string)
}>
type PromptOptions = _PromptOptions & {
  result?: (value: string) => unknown
  choices?: Array<{
    initial?: string
    name: string
  }>
  template?: string
}

const promptUrlValidator = (value: string) => {
  if (!value) return true
  try {
    new URL(value).toString()
    return true
  } catch {
    return 'Invalid URL'
  }
}

export async function applyPrompt (opts: {
  force: boolean
  silent: boolean
  askLevel: ValueOf<typeof AskLevel>
  packageJson: PkgJsonType
}): Promise<void> {
  const {
    askLevel,
    packageJson,
  } = opts
  const promptBase = {
    type: 'input',
    prefix: '>',
    required: false,
    format: (value) => typeof value === 'string' ? String.prototype.trim.call(value) : '',
  } satisfies Partial<PromptOptions>
  const prompts: PromptOptions[] = []
  const firstPrompts = [
    {
      name: 'name',
      message: 'package name',
      initial: packageJson.name,
      required: true,
      validate: packageNameValidator,
    },
    {
      name: 'version',
      message: 'version',
      initial: packageJson.version,
      validate: (value) => typeof value === 'string' ? semverValid(value) !== null : false,
    },
    {
      name: 'description',
      message: 'description',
      initial: packageJson.description,
    },
    {
      name: 'main',
      message: 'entry point',
      initial: packageJson.main,
    },
    {
      name: 'scripts',
      message: 'test command',
      initial: packageJson.scripts?.test,
    },
    {
      name: 'repository',
      message: 'git repository',
      initial: typeof packageJson.repository === 'string' ? packageJson.repository : packageJson?.repository?.url,
      validate: promptUrlValidator,
    },
    {
      type: 'list',
      name: 'keywords',
      message: 'keywords',
      initial: packageJson.keywords?.join(', '),
    },
    {
      name: 'author',
      message: 'author',
      initial: packageJson.author,
    },
    {
      name: 'license',
      message: 'license',
      initial: packageJson.license,
    }] satisfies Array<Partial<PromptOptions>>

  if (askLevel === AskLevel.npm) {
    // Limited prompts
    prompts.push(...(firstPrompts.map((p) => ({ ...promptBase, ...p }))))
  } else {
    if (askLevel > AskLevel.npm) {
      const morePrompts = [
        {
          name: 'bugs',
          message: 'Bugs URL',
          initial: typeof packageJson.bugs === 'string' ? packageJson.bugs : packageJson?.bugs?.url,
        },
        {
          name: 'homepage',
          message: 'Homepage',
          initial: packageJson.homepage,
          validate: promptUrlValidator,
        },
        {
          type: 'list',
          name: 'funding',
          message: 'Funding',
          initial: typeof packageJson.funding === 'string' ? packageJson.funding : packageJson.funding?.join(', '),
          validate: promptUrlValidator,
        },
      ] satisfies Array<Partial<PromptOptions>>
      // Form prompts
      prompts.push({
        name: '_form',
        type: 'form',
        message: 'package.json',
        choices: [
          ...firstPrompts,
          ...morePrompts,
        ],
      }, {
        name: 'private',
        type: 'confirm',
        message: 'Make the package private',
        initial: packageJson.private,
      }, {
        name: 'type',
        type: 'select',
        message: 'Module type',
        initial: packageJson.type ?? 'module',
        choices: [
          { name: 'module' },
          { name: 'commonjs' },
        ],
      })
    }
  }
  const answers = await prompt(prompts.map((p) => ({
    ...promptBase,
    onCancel () {
      // By default, canceling the prompt via Ctrl+c throws an empty string.
      // The custom cancel function prevents that behavior.
      // Otherwise, pnpm CLI would print an error and confuse users.
      // See related issue: https://github.com/enquirer/enquirer/issues/225
      globalInfo('Package.json Init canceled')
      process.exit(0)
    },
    ...p,
  }))) as Record<string, unknown>
  const notMeta = <T extends string>(key: T): key is Exclude<T, `_${string}`> => key[0] !== '_'
  const tryApply = (key: string, value: unknown) => {
    if (key === 'keywords') {
      return typeof value === 'string' ? value.split(',').map((k) => k?.trim()) : undefined
    } else if (key === 'scripts' && typeof value === 'string') {
      return { test: value }
    }
    return value
  }

  const cleanPkgJson = (key: string) => {
    const pkgValue = (packageJson as Record<string, unknown>)[key]
    if (pkgValue === '' ||
    (Array.isArray(pkgValue) && (
      pkgValue.length === 0 || pkgValue.every((v) => v === '')
    )) ||
    (typeof pkgValue === 'object' && pkgValue !== null && (
      Object.keys(pkgValue).length === 0 || Object.values(pkgValue).every((v) => !v)
    ))
    ) {
      delete (packageJson as Record<string, unknown>)[key]
    }
  }

  for (const key of Object.keys(answers) as Array<keyof PkgJsonType | '_form'>) {
    if (key === '_form') {
      for (const [k, v] of Object.entries(answers[key] as Record<string, unknown>)) {
        const value = tryApply(k, v)
        if (value) {
          (packageJson as Record<string, unknown>)[k] = value
        }
        cleanPkgJson(k)
      }
    } else {
      const v = tryApply(key, answers[key])
      if (v && notMeta(key))
        (packageJson as Record<string, unknown>)[key] = v

      cleanPkgJson(key)
    }
  }
}
