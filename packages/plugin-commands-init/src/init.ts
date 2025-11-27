import fs from 'fs'
import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import { packageManager } from '@pnpm/cli-meta'
import { types as allTypes, type Config, type UniversalOptions } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { sortKeysByPriority } from '@pnpm/object.key-sorting'
import { type ProjectManifest } from '@pnpm/types'
import { writeProjectManifest } from '@pnpm/write-project-manifest'
import { pick } from 'ramda'
import renderHelp from 'render-help'
import { parseRawConfig } from './utils.js'

export const rcOptionsTypes = cliOptionsTypes

const NOT_BARE_KEYS = [
  'name',
  'version',
  'description',
  'main',
  'keywords',
  'author',
  'license',
] as const satisfies Array<keyof ProjectManifest>

export function cliOptionsTypes (): Record<string, unknown> {
  return pick([
    'init-bare',
    'init-package-manager',
    'init-type',
  ], allTypes)
}

export const commandNames = ['init']

export const shorthands: Record<string, string> = {
  bare: '--init-bare',
  B: '--init-bare',
}

export function help (): string {
  return renderHelp({
    description: 'Create a package.json file',
    descriptionLists: [
      {
        title: 'Options',
        list: [
          {
            description: 'Set the module system for the package. Defaults to "commonjs".',
            name: '--init-type <commonjs|module>',
          },
          {
            description: 'Pin the project to the current pnpm version by adding a "packageManager" field to package.json',
            name: '--init-package-manager',
          },
          {
            description: `Set private to true and do not set ${NOT_BARE_KEYS.join(', ')}`,
            name: '--init-bare',
          },
        ],
      },
    ],
    url: docsUrl('init'),
    usages: ['pnpm init'],
  })
}

export type InitOptions =
  & Pick<UniversalOptions, 'rawConfig'>
  & Pick<Config, 'cliOptions'>
  & Partial<Pick<Config,
  | 'initBare'
  | 'initPackageManager'
  | 'initType'
  >>

export async function handler (opts: InitOptions, params?: string[]): Promise<string> {
  if (params?.length) {
    throw new PnpmError('INIT_ARG', 'init command does not accept any arguments', {
      hint: `Maybe you wanted to run "pnpm create ${params.join(' ')}"`,
    })
  }
  // Using cwd instead of the dir option because the dir option
  // is set to the first parent directory that has a package.json file
  // But --dir option from cliOptions should be respected.
  const manifestPath = path.join(opts.cliOptions.dir ?? process.cwd(), 'package.json')
  if (fs.existsSync(manifestPath)) {
    throw new PnpmError('PACKAGE_JSON_EXISTS', 'package.json already exists')
  }
  const manifest: ProjectManifest = {
    name: path.basename(process.cwd()),
    version: '1.0.0',
    description: '',
    main: 'index.js',
    scripts: {
      test: 'echo "Error: no test specified" && exit 1',
    },
    keywords: [],
    author: '',
    license: 'ISC',
  }

  if (opts.initType === 'module') {
    manifest.type = opts.initType
  }

  const config = await parseRawConfig(opts.rawConfig)
  const packageJson = { ...manifest, ...config }
  if (opts.initPackageManager) {
    packageJson.packageManager = `pnpm@${packageManager.version}`
  }
  handleInitBare(packageJson, opts)
  const priority = Object.fromEntries([
    'name',
    'version',
    'private',
    'description',
    'main',
    'scripts',
    'keywords',
    'author',
    'license',
    'packageManager',
  ].map((key, index) => [key, index]))
  const sortedPackageJson = sortKeysByPriority({ priority }, packageJson)
  await writeProjectManifest(manifestPath, sortedPackageJson, {
    indent: 2,
  })
  return `Wrote to ${manifestPath}

${JSON.stringify(sortedPackageJson, null, 2)}`
}

function handleInitBare (manifest: ProjectManifest, opts: Pick<InitOptions, 'initBare'>): void {
  if (opts.initBare) {
    for (const key of NOT_BARE_KEYS) {
      delete manifest[key]
    }
    manifest.private = true
  }
}
