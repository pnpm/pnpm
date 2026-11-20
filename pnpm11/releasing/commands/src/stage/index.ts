import { types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { pick } from 'ramda'

import * as publishCommand from '../publish/publish.js'
import { stageApprove } from './approve.js'
import { stageDownload } from './download.js'
import { help } from './help.js'
import { stageList } from './list.js'
import { stagePublish } from './publish.js'
import { stageReject } from './reject.js'
import { STAGE_SUBCOMMANDS, type StageOptions, type StageSubcommand } from './types.js'
import { stageView } from './view.js'

export { help }

export function rcOptionsTypes (): Record<string, unknown> {
  return {
    ...publishCommand.rcOptionsTypes(),
    ...pick([
      'registry',
    ], allTypes),
  }
}

export function cliOptionsTypes (): Record<string, unknown> {
  return publishCommand.cliOptionsTypes()
}

export const commandNames = ['stage']

export const completion = async (_cliOpts: Record<string, unknown>, params: string[]): Promise<Array<{ name: string }>> => {
  if (params.length > 0) return []
  return STAGE_SUBCOMMANDS.map((name) => ({ name }))
}

export async function handler (
  opts: StageOptions,
  params: string[]
): Promise<{ exitCode?: number, output?: string } | string | undefined> {
  const subcommand = params[0] as StageSubcommand | undefined
  const subcommandParams = params.slice(1)

  switch (subcommand) {
    case 'publish':
      return stagePublish(opts, subcommandParams)
    case 'list':
      return stageList(opts, subcommandParams)
    case 'view':
      return stageView(opts, subcommandParams)
    case 'approve':
      return stageApprove(opts, subcommandParams)
    case 'reject':
      return stageReject(opts, subcommandParams)
    case 'download':
      return stageDownload(opts, subcommandParams)
    case undefined:
      throw new PnpmError('STAGE_SUBCOMMAND_REQUIRED', 'Stage subcommand is required', {
        hint: `Use one of: ${STAGE_SUBCOMMANDS.join(', ')}`,
      })
    default:
      throw new PnpmError('STAGE_UNKNOWN_SUBCOMMAND', `Unknown stage subcommand "${subcommand}"`, {
        hint: `Use one of: ${STAGE_SUBCOMMANDS.join(', ')}`,
      })
  }
}
