import {sync as runScriptSync} from '../runScript'

export default function runNpm (args: string[]) {
  const result = runScriptSync('npm', args, {
    cwd: process.cwd(),
    stdio: 'inherit',
  })
  process.exit(result.status)
}
