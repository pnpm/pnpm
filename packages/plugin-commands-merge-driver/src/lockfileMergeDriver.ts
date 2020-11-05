import { readWantedLockfile, writeWantedLockfile } from '@pnpm/lockfile-file'
import mergeLockfileChanges from '@pnpm/merge-lockfile-changes'
import execa = require('execa')
import path = require('path')
import renderHelp = require('render-help')

export const commandNames = ['lockfile-merge-driver']

export const cliOptionsTypes = () => ({})

export const rcOptionsTypes = () => ({})

export function help () {
  return renderHelp({
    usages: [],
  })
}

export async function handler (opts: {}, [ours, base, theirs, mergedFilePath]: string[]) {
  const mergedLockfile = mergeLockfileChanges(
    (await readWantedLockfile(path.dirname(ours), { ignoreIncompatible: true }))!,
    (await readWantedLockfile(path.dirname(theirs), { ignoreIncompatible: true }))!
  )

  const cwd = path.dirname(mergedFilePath)
  await writeWantedLockfile(cwd, mergedLockfile)
  let main = require.main!.filename
  if (path.basename(main) !== 'pnpm.js') {
    // This will happen only during testing
    main = path.join(__dirname, '../../pnpm/bin/pnpm.js')
  }
  execa.sync('node', [
    main,
    'install',
    '--lockfile-only',
    '--no-frozen-lockfile',
    '--force', // this will force a full resolution
    '--ignore-scripts',
  ], {
    cwd,
    stdio: 'inherit',
  })
}
