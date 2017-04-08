import {sync as runScriptSync} from '../runScript'

export default function runNpm (args: string[]) {
  const result = runScriptSync('npm', args, {
    cwd: process.cwd(),
    stdio: 'inherit',
    userAgent: undefined,
  })
  process.exit(result.status)
}
