import { sync as runScriptSync } from '../runScript'

export default function runNpm (args: string[]) {
  return runScriptSync('npm', args, {
    cwd: process.cwd(),
    stdio: 'inherit',
    userAgent: undefined,
  })
}
