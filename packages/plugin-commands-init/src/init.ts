import fs from 'node:fs'
import path from 'node:path'
import { docsUrl } from '@pnpm/cli-utils'
import { packageManager } from '@pnpm/cli-meta'
import { types as allTypes, type Config, type UniversalOptions } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { sortKeysByPriority } from '@pnpm/object.key-sorting'
import { type ProjectManifest } from '@pnpm/types'
import { writeProjectManifest } from '@pnpm/write-project-manifest'
import pick from 'ramda/src/pick'
import renderHelp from 'render-help'
import { parseRawConfig } from './utils'

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes (): Record<string, unknown> {
  return pick(['init-type', 'init-package-manager'], allTypes)
}

export const commandNames = ['init']

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
        ],
      },
    ],
    url: docsUrl('init'),
    usages: ['pnpm init'],
  })
}

export async function handler (
  opts: Pick<UniversalOptions, 'rawConfig'> & Pick<Config, 'cliOptions'> & Partial<Pick<Config, 'initPackageManager' | 'initType'>>,
  params?: string[]
): Promise<string> {
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
  const priority = Object.fromEntries([
    'name',
    'version',
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
