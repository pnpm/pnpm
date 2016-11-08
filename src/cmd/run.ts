import {PnpmOptions} from '../types'
import {sync as runScriptSync} from '../runScript'

export default function (input: string[], opts: PnpmOptions) {
  runScriptSync('npm', ['run'].concat(input), {
    cwd: process.cwd(),
    stdio: 'inherit',
  })
}
