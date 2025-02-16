import fs from 'fs'
import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import { packageManager } from '@pnpm/cli-meta'
import { type Config, type UniversalOptions } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { sortKeysByPriority } from '@pnpm/object.key-sorting'
import { type ProjectManifest } from '@pnpm/types'
import { writeProjectManifest } from '@pnpm/write-project-manifest'
import renderHelp from 'render-help'
import { parseRawConfig } from './utils'

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes (): Record<string, unknown> {
  return {}
}

export const commandNames = ['init']

export function help (): string {
  return renderHelp({
    description: 'Create a package.json file',
    descriptionLists: [],
    url: docsUrl('init'),
    usages: ['pnpm init'],
  })
}

export async function handler (
  opts: Pick<UniversalOptions, 'rawConfig'> & Pick<Config, 'cliOptions'> & Partial<Pick<Config, 'initPackageManager'>>,
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

${JSON.stringify(packageJson, null, 2)}`
}
