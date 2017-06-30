import {PnpmOptions} from 'supi'
import {sync as runScriptSync} from '../runScript'

export default function (input: string[], opts: PnpmOptions) {
  return runScriptSync('npm', ['run'].concat(input), {
    cwd: process.cwd(),
    stdio: 'inherit',
    userAgent: undefined,
  })
}
