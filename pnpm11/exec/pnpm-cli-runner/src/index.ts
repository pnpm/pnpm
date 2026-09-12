import { detectIfCurrentPkgIsExecutable } from '@pnpm/cli.meta'
import { sync as execSync } from 'execa'

export interface RunPnpmCliOptions {
  cwd: string
  reporter?: string
}

export function runPnpmCli (command: string[], { cwd, reporter }: RunPnpmCliOptions): void {
  const execOpts = {
    cwd,
    stdio: 'inherit' as const,
  }
  const cliCommand = reporter ? [...command, `--reporter=${reporter}`] : command
  // Re-invoke the pnpm that is running now, so the child runs the same version.
  // The `@pnpm/exe` single-file build is its own executable; every other
  // install method runs a script that `process.argv[1]` points at, whatever it
  // is named. Only a process started without an entry script (`node -e`) has to
  // guess and take whichever pnpm is on PATH.
  if (detectIfCurrentPkgIsExecutable()) {
    execSync(process.execPath, cliCommand, execOpts)
  } else if (process.argv[1]) {
    execSync(process.execPath, [process.argv[1], ...cliCommand], execOpts)
  } else {
    execSync('pnpm', cliCommand, execOpts)
  }
}
