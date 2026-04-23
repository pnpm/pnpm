import path from 'node:path'

import { pick } from 'ramda'
import { renderHelp } from 'render-help'
import { docsUrl } from '@pnpm/cli.utils'
import { type Config, types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { readPackageJsonFromDirRawSync } from '@pnpm/pkg-manifest.reader'
import { writeProjectManifest } from '@pnpm/workspace.project-manifest-writer'

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes (): Record<string, unknown> {
  return pick([
    'dir',
  ], allTypes)
}

export const commandNames = ['set-script', 'ss']

export function help (): string {
  return renderHelp({
    description: 'Set a script in package.json',
    usages: ['pnpm set-script <name> <command>'],
    url: docsUrl('set-script'),
  })
}

export async function handler (
  opts: Pick<Config, 'dir'>,
  params: string[]
): Promise<void> {
  if (params.length < 2) {
    throw new PnpmError('SET_SCRIPT_MISSING_ARGS', 'Missing script name or command', {
      hint: help(),
    })
  }

  const [name, ...commandParts] = params
  const command = commandParts.join(' ')

  const manifest = readPackageJsonFromDirRawSync(opts.dir)

  if (!manifest.scripts) {
    manifest.scripts = {}
  }
  manifest.scripts[name] = command

  const manifestPath = path.join(opts.dir, 'package.json')
  await writeProjectManifest(manifestPath, manifest)
}
