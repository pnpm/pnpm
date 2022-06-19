import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import { types as allTypes } from '@pnpm/config'
import pick from 'ramda/src/pick'
import execa from 'safe-execa'
import escapeStringRegexp from 'escape-string-regexp'
import renderHelp from 'render-help'

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes () {
  return pick([], allTypes)
}

export const commandNames = ['patch-commit']

export function help () {
  return renderHelp({
    description: 'Generate a patch out of a directory',
    descriptionLists: [],
    url: docsUrl('patch-commit'),
    usages: ['pnpm patch-commit <patchDir>'],
  })
}

export async function handler (opts: {}, params: string[]) {
  const baseDir = params[0]
  return diffFolders(path.join(baseDir, 'source'), path.join(baseDir, 'user'))
}

async function diffFolders (folderA: string, folderB: string) {
  const folderAN = folderA.replace(/\\/g, `/`);
  const folderBN = folderB.replace(/\\/g, `/`);
  let stdout!: string
  let stderr!: string

  try {
    const result = await execa(`git`, [`-c`, `core.safecrlf=false`, `diff`, `--src-prefix=a/`, `--dst-prefix=b/`, `--ignore-cr-at-eol`, `--full-index`, `--no-index`, `--text`, folderAN, folderBN], {
      cwd: process.cwd(),
      env: {
        ...process.env,
        //#region Predictable output
        // These variables aim to ignore the global git config so we get predictable output
        // https://git-scm.com/docs/git#Documentation/git.txt-codeGITCONFIGNOSYSTEMcode
        GIT_CONFIG_NOSYSTEM: `1`,
        HOME: ``,
        XDG_CONFIG_HOME: ``,
        USERPROFILE: ``,
        //#endregion
      },
    });
    stdout = result.stdout
    stderr = result.stderr

  } catch (err: any) {
    stdout = err.stdout
    stderr = err.stderr
  }
  // we cannot rely on exit code, because --no-index implies --exit-code
  // i.e. git diff will exit with 1 if there were differences
  if (stderr.length > 0)
    throw new Error(`Unable to diff directories. Make sure you have a recent version of 'git' available in PATH.\nThe following error was reported by 'git':\n${stderr}`);


  const normalizePath = folderAN.startsWith(`/`)
    ? (p: string) => p.slice(1)
    : (p: string) => p;

  return stdout
    .replace(new RegExp(`(a|b)(${escapeStringRegexp(`/${normalizePath(folderAN)}/`)})`, `g`), `$1/`)
    .replace(new RegExp(`(a|b)${escapeStringRegexp(`/${normalizePath(folderBN)}/`)}`, `g`), `$1/`)
    .replace(new RegExp(escapeStringRegexp(`${folderAN}/`), `g`), ``)
    .replace(new RegExp(escapeStringRegexp(`${folderBN}/`), `g`), ``);
}
