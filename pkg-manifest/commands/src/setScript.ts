import { docsUrl, readProjectManifest } from '@pnpm/cli.utils'
import { types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { setObjectValueByPropertyPath } from '@pnpm/object.property-path'
import { renderHelp } from 'render-help'

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes (): Record<string, unknown> {
  const types = allTypes as Record<string, unknown>
  return { dir: types['dir'] }
}

export const commandNames = ['set-script', 'ss']

export async function handler (
  opts: { dir: string },
  params: string[]
): Promise<void> {
  if (params.length < 2) {
    throw new PnpmError('SET_SCRIPT_MISSING_ARGS', 'Missing script name or command', {
      hint: help(),
    })
  }

  const [name, ...commandParts] = params
  const command = commandParts.join(' ')

  const { manifest, writeProjectManifest } = await readProjectManifest(opts.dir)
  setObjectValueByPropertyPath(manifest as unknown as Record<string, unknown>, ['scripts', name], command)
  await writeProjectManifest(manifest)
}

export function help (): string {
  return renderHelp({
    description: 'Set a script in package.json',
    usages: ['pnpm set-script <name> <command>'],
    url: docsUrl('set-script'),
  })
}
