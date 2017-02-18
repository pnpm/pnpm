import crossSpawn = require('cross-spawn')
import {PnpmOptions} from '../types'
import verifyCmd from './verify'

export default async function (input: string[], opts: PnpmOptions) {
  await verifyCmd(input, opts)

  crossSpawn.sync('npm', process.argv.slice(2), { stdio: 'inherit' })
}
