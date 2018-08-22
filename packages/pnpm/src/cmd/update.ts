import {PnpmOptions} from '../types'
import installCmd from './install'

export default async function (
  input: string[],
  opts: PnpmOptions,
) {
  return installCmd(input, {...opts, update: true, allowNew: false})
}
