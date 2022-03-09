import fs from 'fs'
import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import { UniversalOptions } from '@pnpm/config'
import PnpmError from '@pnpm/error'
import writeProjectManifest from '@pnpm/write-project-manifest'
import renderHelp from 'render-help'
import { parseRawConfig } from './utils'

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes () {
  return {}
}

export const commandNames = ['init']

export function help () {
  return renderHelp({
    description: 'Create a package.json file',
    descriptionLists: [],
    url: docsUrl('init'),
    usages: ['pnpm init'],
  })
}

export async function handler (
  opts: Pick<UniversalOptions, 'dir' | 'rawConfig'>
) {
  const manifestPath = path.join(opts.dir, 'package.json')
  if (fs.existsSync(manifestPath)) {
    throw new PnpmError('PACKAGE_JSON_EXISTS', 'package.json already exists')
  }
  const manifest = {
    name: path.basename(opts.dir),
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
  await writeProjectManifest(manifestPath, packageJson, {
    indent: 2,
  })
  return `Wrote to ${path.join(opts.dir, 'package.json')}

${JSON.stringify(packageJson, null, 2)}`
}
