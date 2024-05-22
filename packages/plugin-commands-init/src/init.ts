import fs from 'fs'
import path from 'path'
import { type UniversalOptions } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { writeProjectManifest } from '@pnpm/write-project-manifest'
import { findWorkspaceDir } from '@pnpm/find-workspace-dir'
import { readWorkspaceManifest } from '@pnpm/workspace.read-manifest'
import { logger } from '@pnpm/logger'
import { getMetaOptions, parseRawConfig } from './utils'
import type { OptionsRaw } from './options'
export { help, rcOptionsTypes, cliOptionsTypes } from './options'

export const commandNames = ['init', 'innit']

const getManifestDefaults = () => ({
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
})
export type ManifestDefaults = ReturnType<typeof getManifestDefaults>

export async function handler (
  opts: Pick<UniversalOptions, 'rawConfig'> & {
    rawConfig: OptionsRaw
  },
  params?: string[]
): Promise<string> {
  const { silent, force, initAsk, workspaceUpdate } = getMetaOptions(opts.rawConfig)
  if (params?.length) {
    if (force) !silent || logger.warn({
      message: 'Ignoring invalid arguments because --force is used',
      prefix: process.cwd(),
    })
    else throw new PnpmError('INIT_ARG', 'init command does not accept any arguments', {
      hint: `Maybe you wanted to run "pnpm create ${params.join(' ')}"`,
    })
  }
  // Using cwd instead of the dir option because the dir option
  // is set to the first parent directory that has a package.json file.
  const manifestPath = path.join(process.cwd(), 'package.json')
  if (fs.existsSync(manifestPath) && !force) {
    if (force) !silent || logger.warn({
      message: 'Overwriting existing package.json because --force is used',
      prefix: process.cwd(),
    })
    else throw new PnpmError('PACKAGE_JSON_EXISTS', 'package.json already exists')
  }
  const manifest = getManifestDefaults()
  const config = await parseRawConfig(opts.rawConfig, manifest)
  const packageJson = { ...(initAsk ? {} : manifest), ...config }
  await writeProjectManifest(manifestPath, packageJson, {
    indent: 2,
  })

  let addedToWorkspace: string | false = false
  if (workspaceUpdate) {
    const workspaceDir = await findWorkspaceDir(process.cwd())
    const workspaceManifestPath = workspaceDir && path.join(workspaceDir, 'pnpm-workspace.yaml')
    const workspaceManifest = workspaceManifestPath && await readWorkspaceManifest(workspaceDir)
    const relativeWorkspacePath = workspaceManifestPath && path.relative(path.resolve(workspaceDir), path.resolve()).split(path.sep).join('/')
    if (workspaceManifest && workspaceManifest.packages && relativeWorkspacePath) {
      if (!workspaceManifest.packages.includes(relativeWorkspacePath)) {
        workspaceManifest.packages.push(relativeWorkspacePath)
        await fs.promises.writeFile(workspaceManifestPath, `packages:\n${workspaceManifest.packages.map((pkgPath) => `  - ${pkgPath}`).join('\n')}\n`)
        addedToWorkspace = workspaceManifestPath
      }
    }
  }
  const workspaceMessage = !addedToWorkspace
    ? ''
    : `Added to workspace at ${addedToWorkspace}

`
  return `${workspaceMessage}Wrote to ${manifestPath}

${JSON.stringify(packageJson, null, 2)}`
}
