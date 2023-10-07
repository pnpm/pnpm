import fs from 'fs'
import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import { type Config, types as allTypes } from '@pnpm/config'
import { install } from '@pnpm/plugin-commands-installation'
import { readPackageJsonFromDir } from '@pnpm/read-package-json'
import { tryReadProjectManifest } from '@pnpm/read-project-manifest'
import glob from 'fast-glob'
import normalizePath from 'normalize-path'
import pick from 'ramda/src/pick'
import equals from 'ramda/src/equals'
import execa from 'safe-execa'
import escapeStringRegexp from 'escape-string-regexp'
import renderHelp from 'render-help'
import tempy from 'tempy'
import { writePackage } from './writePackage'
import { parseWantedDependency } from '@pnpm/parse-wanted-dependency'
import packlist from 'npm-packlist'

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes () {
  return pick(['patches-dir'], allTypes)
}

export const commandNames = ['patch-commit']

export function help () {
  return renderHelp({
    description: 'Generate a patch out of a directory',
    descriptionLists: [{
      title: 'Options',
      list: [
        {
          description: 'The generated patch file will be saved to this directory',
          name: '--patches-dir',
        },
      ],
    }],
    url: docsUrl('patch-commit'),
    usages: ['pnpm patch-commit <patchDir>'],
  })
}

export async function handler (opts: install.InstallCommandOptions & Pick<Config, 'patchesDir' | 'rootProjectManifest'>, params: string[]) {
  const userDir = params[0]
  const lockfileDir = opts.lockfileDir ?? opts.dir ?? process.cwd()
  const patchesDirName = normalizePath(path.normalize(opts.patchesDir ?? 'patches'))
  const patchesDir = path.join(lockfileDir, patchesDirName)
  await fs.promises.mkdir(patchesDir, { recursive: true })
  const patchedPkgManifest = await readPackageJsonFromDir(userDir)
  const pkgNameAndVersion = `${patchedPkgManifest.name}@${patchedPkgManifest.version}`
  const srcDir = tempy.directory()
  await writePackage(parseWantedDependency(pkgNameAndVersion), srcDir, opts)

  const patchedPkgDir = await preparePkgFilesForDiff(userDir)
  const patchContent = await diffFolders(srcDir, patchedPkgDir)

  if (!patchContent.length) {
    return `No changes were found to the following directory: ${userDir}`
  }

  const patchFileName = pkgNameAndVersion.replace('/', '__')
  await fs.promises.writeFile(path.join(patchesDir, `${patchFileName}.patch`), patchContent, 'utf8')
  const { writeProjectManifest, manifest } = await tryReadProjectManifest(lockfileDir)

  const rootProjectManifest = (!opts.sharedWorkspaceLockfile ? manifest : (opts.rootProjectManifest ?? manifest)) ?? {}

  if (!rootProjectManifest.pnpm) {
    rootProjectManifest.pnpm = {
      patchedDependencies: {},
    }
  } else if (!rootProjectManifest.pnpm.patchedDependencies) {
    rootProjectManifest.pnpm.patchedDependencies = {}
  }
  rootProjectManifest.pnpm.patchedDependencies![pkgNameAndVersion] = `${patchesDirName}/${patchFileName}.patch`
  await writeProjectManifest(rootProjectManifest)

  if (opts?.selectedProjectsGraph?.[lockfileDir]) {
    opts.selectedProjectsGraph[lockfileDir].package.manifest = rootProjectManifest
  }

  if (opts?.allProjectsGraph?.[lockfileDir].package.manifest) {
    opts.allProjectsGraph[lockfileDir].package.manifest = rootProjectManifest
  }

  return install.handler({
    ...opts,
    rootProjectManifest,
    rawLocalConfig: {
      ...opts.rawLocalConfig,
      'frozen-lockfile': false,
    },
  })
}

async function diffFolders (folderA: string, folderB: string) {
  const folderAN = folderA.replace(/\\/g, '/')
  const folderBN = folderB.replace(/\\/g, '/')
  let stdout!: string
  let stderr!: string

  try {
    const result = await execa('git', ['-c', 'core.safecrlf=false', 'diff', '--src-prefix=a/', '--dst-prefix=b/', '--ignore-cr-at-eol', '--irreversible-delete', '--full-index', '--no-index', '--text', folderAN, folderBN], {
      cwd: process.cwd(),
      env: {
        ...process.env,
        // #region Predictable output
        // These variables aim to ignore the global git config so we get predictable output
        // https://git-scm.com/docs/git#Documentation/git.txt-codeGITCONFIGNOSYSTEMcode
        GIT_CONFIG_NOSYSTEM: '1',
        HOME: '',
        XDG_CONFIG_HOME: '',
        USERPROFILE: '',
        // #endregion
      },
      stripFinalNewline: false,
    })
    stdout = result.stdout
    stderr = result.stderr
  } catch (err: any) { // eslint-disable-line
    stdout = err.stdout
    stderr = err.stderr
  }
  // we cannot rely on exit code, because --no-index implies --exit-code
  // i.e. git diff will exit with 1 if there were differences
  if (stderr.length > 0)
    throw new Error(`Unable to diff directories. Make sure you have a recent version of 'git' available in PATH.\nThe following error was reported by 'git':\n${stderr}`)

  return stdout
    .replace(new RegExp(`(a|b)(${escapeStringRegexp(`/${removeTrailingAndLeadingSlash(folderAN)}/`)})`, 'g'), '$1/')
    .replace(new RegExp(`(a|b)${escapeStringRegexp(`/${removeTrailingAndLeadingSlash(folderBN)}/`)}`, 'g'), '$1/')
    .replace(new RegExp(escapeStringRegexp(`${folderAN}/`), 'g'), '')
    .replace(new RegExp(escapeStringRegexp(`${folderBN}/`), 'g'), '')
    .replace(/\n\\ No newline at end of file\n$/, '\n')
}

function removeTrailingAndLeadingSlash (p: string) {
  if (p[0] === '/' || p.endsWith('/')) {
    return p.replace(/^\/|\/$/g, '')
  }
  return p
}

/**
 * Link files from the source directory to a new temporary directory,
 * but only if not all files in the source directory should be included in the package.
 * If all files should be included, return the original source directory without creating any links.
 * This is required in order for the diff to not include files that are not part of the package.
 */
async function preparePkgFilesForDiff (src: string): Promise<string> {
  const files = Array.from(new Set((await packlist({ path: src })).map((f) => path.join(f))))
  // If there are no extra files in the source directories, then there is no reason
  // to copy.
  if (await areAllFilesInPkg(files, src)) {
    return src
  }
  const dest = tempy.directory()
  await Promise.all(
    files.map(async (file) => {
      const srcFile = path.join(src, file)
      const destFile = path.join(dest, file)
      const destDir = path.dirname(destFile)
      await fs.promises.mkdir(destDir, { recursive: true })
      await fs.promises.link(srcFile, destFile)
    })
  )
  return dest
}

async function areAllFilesInPkg (files: string[], basePath: string) {
  const allFiles = await glob('**', {
    cwd: basePath,
  })
  return equals(allFiles.sort(), files.sort())
}
