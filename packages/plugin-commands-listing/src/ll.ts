import R = require('ramda')
import * as list from './list'

export const commandNames = ['ll', 'la']

export const rcOptionsTypes = list.rcOptionsTypes

export function cliOptionsTypes () {
  return R.omit(['long'], list.cliOptionsTypes())
}

export const help = list.help()

export function handler (
  opts: list.ListCommandOptions,
  params: string[]
) {
  return list.handler({ ...opts, long: true }, params)
}
