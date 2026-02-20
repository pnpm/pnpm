import fs from 'fs'
import os from 'os'
import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import { type Config, types as allTypes } from '@pnpm/config'
import { createShortHash } from '@pnpm/crypto.hash'
import { PnpmError } from '@pnpm/error'
import { packlist } from '@pnpm/fs.packlist'
import { globalWarn } from '@pnpm/logger'
import { install } from '@pnpm/plugin-commands-installation'
import { readPackageJsonFromDir } from '@pnpm/read-package-json'
import { getStorePath } from '@pnpm/store-path'
import { type ProjectRootDir } from '@pnpm/types'
import { glob } from 'tinyglobby'
import normalizePath from 'normalize-path'
import { pick, equals } from 'ramda'
import execa from 'safe-execa'
import escapeStringRegexp from 'escape-string-regexp'
import makeEmptyDir from 'make-empty-dir'
import renderHelp from 'render-help'
import { type WritePackageOptions, writePackage } from './writePackage.js'
import { type ParseWantedDependencyResult, parseWantedDependency } from '@pnpm/parse-wanted-dependency'
import { type GetPatchedDependencyOptions, getVersionsFromLockfile } from './getPatchedDependency.js'
import { readEditDirState } from './stateFile.js'
import { updatePatchedDependencies } from './updatePatchedDependencies.js'

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes (): Record<string, unknown> {
  return pick(['patches-dir'], allTypes)
}

export const commandNames = ['patch-commit']

export function help (): string {
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

type PatchCommitCommandOptions = install.InstallCommandOptions & Pick<Config, 'patchesDir' | 'rootProjectManifest' | 'rootProjectManifestDir' | 'patchedDependencies'>

export async function handler (opts: PatchCommitCommandOptions, params: string[]): Promise<string | undefined> {
  const userDir = params[0]
  const lockfileDir = (opts.lockfileDir ?? opts.dir ?? process.cwd()) as ProjectRootDir
  const patchesDirName = normalizePath(path.normalize(opts.patchesDir ?? 'patches'))
  const patchesDir = path.join(lockfileDir, patchesDirName)
  const patchedPkgManifest = await readPackageJsonFromDir(userDir)
  const editDir = path.resolve(opts.dir, userDir)
  const stateValue = readEditDirState({
    editDir,
    modulesDir: path.join(lockfileDir, opts.modulesDir ?? 'node_modules'),
  })
  if (!stateValue) {
    throw new PnpmError('INVALID_PATCH_DIR', `${userDir} is not a valid patch directory`, {
      hint: 'A valid patch directory should be created by `pnpm patch`',
    })
  }
  const { applyToAll } = stateValue
  const nameAndVersion = `${patchedPkgManifest.name}@${patchedPkgManifest.version}`
  const patchKey = applyToAll ? patchedPkgManifest.name : nameAndVersion
  let gitTarballUrl: string | undefined
  if (!applyToAll) {
    gitTarballUrl = await getGitTarballUrlFromLockfile({
      alias: patchedPkgManifest.name,
      bareSpecifier: patchedPkgManifest.version || undefined,
    }, {
      lockfileDir,
      modulesDir: opts.modulesDir,
      virtualStoreDir: opts.virtualStoreDir,
    })
  }
  const patchedPkg = parseWantedDependency(gitTarballUrl ? `${patchedPkgManifest.name}@${gitTarballUrl}` : nameAndVersion)
  const patchedPkgDir = await preparePkgFilesForDiff(userDir)
  const patchContent = await getPatchContent({
    patchedPkg,
    patchedPkgDir,
    tmpName: createShortHash(editDir),
  }, opts)
  if (patchedPkgDir !== userDir) {
    fs.rmSync(patchedPkgDir, { recursive: true })
  }

  if (!patchContent.length) {
    return `No changes were found to the following directory: ${userDir}`
  }
  await fs.promises.mkdir(patchesDir, { recursive: true })

  const patchFileName = patchKey.replace('/', '__')
  await fs.promises.writeFile(path.join(patchesDir, `${patchFileName}.patch`), patchContent, 'utf8')

  const patchedDependencies = {
    ...opts.patchedDependencies,
    [patchKey]: `${patchesDirName}/${patchFileName}.patch`,
  }
  await updatePatchedDependencies(patchedDependencies, {
    ...opts,
    workspaceDir: opts.workspaceDir ?? opts.rootProjectManifestDir,
  })

  return install.handler({
    ...opts,
    patchedDependencies,
    rawLocalConfig: {
      ...opts.rawLocalConfig,
      'frozen-lockfile': false,
    },
  }) as Promise<undefined>
}

interface GetPatchContentContext {
  patchedPkg: ParseWantedDependencyResult
  patchedPkgDir: string
  tmpName: string
}

type GetPatchContentOptions = Pick<PatchCommitCommandOptions, 'dir' | 'pnpmHomeDir' | 'storeDir'> & WritePackageOptions

async function getPatchContent (ctx: GetPatchContentContext, opts: GetPatchContentOptions): Promise<string> {
  const storeDir = await getStorePath({
    pkgRoot: opts.dir,
    storePath: opts.storeDir,
    pnpmHomeDir: opts.pnpmHomeDir,
  })
  const srcDir = path.join(storeDir, 'tmp', 'patch-commit', ctx.tmpName)
  await writePackage(ctx.patchedPkg, srcDir, opts)
  const patchContent = await diffFolders(srcDir, ctx.patchedPkgDir)
  try {
    fs.rmSync(srcDir, { recursive: true })
  } catch (error) {
    globalWarn(`Failed to clean up temporary directory at ${srcDir} with error: ${String(error)}`)
  }
  return patchContent
}

async function diffFolders (folderA: string, folderB: string): Promise<string> {
  const folderAN = folderA.replace(/\\/g, '/')
  const folderBN = folderB.replace(/\\/g, '/')
  let stdout!: string
  let stderr!: string

  try {
    const result = await execa('git', ['-c', 'core.safecrlf=false', 'diff', '--src-prefix=a/', '--dst-prefix=b/', '--ignore-cr-at-eol', '--irreversible-delete', '--full-index', '--no-index', '--text', '--no-ext-diff', '--no-color', folderAN, folderBN], {
      cwd: process.cwd(),
      env: {
        ...process.env,
        // #region Predictable output
        // These variables aim to ignore the global git config so we get predictable output
        // https://git-scm.com/docs/git#Documentation/git.txt-codeGITCONFIGNOSYSTEMcode
        GIT_CONFIG_NOSYSTEM: '1',
        // Redirect the global git config to the null device instead of setting
        // HOME to an empty string. An empty HOME causes git to resolve '~' as
        // '/' (root), which triggers a "Permission denied" warning when git
        // tries to access '/.config/git/attributes', making pnpm throw an
        // error because any stderr output is treated as a failure.
        // We do not set XDG_CONFIG_HOME to avoid the same issue: an empty
        // value would make git resolve paths like /git/config and /git/attributes.
        GIT_CONFIG_GLOBAL: os.devNull,
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
    .replace(/^diff --git a\/.*\.DS_Store b\/.*\.DS_Store[\s\S]+?(?=^diff --git)/gm, '')
    .replace(/^diff --git a\/.*\.DS_Store b\/.*\.DS_Store[\s\S]*$/gm, '')
}

function removeTrailingAndLeadingSlash (p: string): string {
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
  const files = Array.from(new Set((await packlist(src)).map((f) => path.join(f))))
  // If there are no extra files in the source directories, then there is no reason
  // to copy.
  if (await areAllFilesInPkg(files, src)) {
    return src
  }
  const dest = `${src}_tmp`
  await makeEmptyDir(dest)
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

async function areAllFilesInPkg (files: string[], basePath: string): Promise<boolean> {
  const allFiles = await glob('**', {
    cwd: basePath,
    expandDirectories: false,
  })
  return equals(allFiles.sort(), files.sort())
}

async function getGitTarballUrlFromLockfile (dep: ParseWantedDependencyResult, opts: GetPatchedDependencyOptions): Promise<string | undefined> {
  const { preferredVersions } = await getVersionsFromLockfile(dep, opts)
  return preferredVersions[0]?.gitTarballUrl
}
