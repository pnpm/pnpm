import { PnpmOptions } from '../types'
import install from './install'
import { test } from './run'

export default async function (input: string[], opts: PnpmOptions) {
  await install(input, opts)
  await test(input, opts)
}
