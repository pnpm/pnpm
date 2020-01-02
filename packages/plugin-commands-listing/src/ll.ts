import R = require('ramda')
import * as list from './list'

export const commandNames = ['ll', 'la']

export const rcOptionsTypes = list.rcOptionsTypes

export function cliOptionsTypes () {
  return R.omit(['long'], list.cliOptionsTypes())
}

export const help = list.help()

export function handler (
  args: string[],
  opts: list.ListCommandOptions,
) {
  return list.handler(args, { ...opts, long: true })
}
