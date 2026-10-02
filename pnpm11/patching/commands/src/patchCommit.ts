import fs from 'node:fs'
import path from 'node:path'

import { docsUrl } from '@pnpm/cli.utils'
import { type Config, type ConfigContext, types as allTypes } from '@pnpm/config.reader'
import { createShortHash } from '@pnpm/crypto.hash'
import { isError, PnpmError } from '@pnpm/error'
import { packlist } from '@pnpm/fs.packlist'
import { install } from '@pnpm/installing.commands'
import { type PackageSnapshot, readWantedLockfile, writeWantedLockfile } from '@pnpm/lockfile.fs'
import { pruneSharedLockfile } from '@pnpm/lockfile.pruner'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import { globalWarn } from '@pnpm/logger'
import { readPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import { parseWantedDependency, type ParseWantedDependencyResult } from '@pnpm/resolving.parse-wanted-dependency'
import { getStorePath } from '@pnpm/store.path'
import type { PackageManifest, ProjectRootDir } from '@pnpm/types'
import escapeStringRegexp from 'escape-string-regexp'
import { makeEmptyDir } from 'make-empty-dir'
import normalizePath from 'normalize-path'
import { equals, pick } from 'ramda'
import { renderHelp } from 'render-help'
import { safeExeca as execa } from 'safe-execa'
import { glob } from 'tinyglobby'

import { type GetPatchedDependencyOptions, getVersionsFromLockfile } from './getPatchedDependency.js'
import { resolvePatchDir } from './resolvePatchDir.js'
import { updatePatchedDependencies } from './updatePatchedDependencies.js'
import { writePackage, type WritePackageOptions } from './writePackage.js'

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes (): Record<string, unknown> {
  return pick(['patches-dir'], allTypes)
}

export const commandNames = ['patch-commit']

export const recursiveByDefault = true

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

type PatchCommitCommandOptions = install.InstallCommandOptions &
  Pick<Config, 'patchesDir' | 'patchedDependencies'> &
  Partial<Pick<Config, 'useGitBranchLockfile' | 'mergeGitBranchLockfiles'>> &
  Pick<ConfigContext, 'rootProjectManifest' | 'rootProjectManifestDir'>

export async function handler (opts: PatchCommitCommandOptions, params: string[]): Promise<string | undefined> {
  if (!params[0]) {
    throw new PnpmError('MISSING_PACKAGE_NAME', '`pnpm patch-commit` requires the patch directory or package name')
  }
  const userParam = params[0]
  const lockfileDir = (opts.lockfileDir ?? opts.dir ?? process.cwd()) as ProjectRootDir
  const { editDir, stateValue: { applyToAll } } = await resolvePatchDir(userParam, {
    dir: opts.dir,
    lockfileDir,
    modulesDir: path.join(lockfileDir, opts.modulesDir ?? 'node_modules'),
  })
  const patchedPkgManifest = await readPackageJsonFromDir(editDir)
  const patchedPkg = await getPatchedPkgToDiff(patchedPkgManifest, {
    applyToAll,
    lockfileDir,
    modulesDir: opts.modulesDir,
    virtualStoreDir: opts.virtualStoreDir,
  })
  const patchContent = await diffEditDir(editDir, patchedPkg, opts)
  if (!patchContent.length) {
    return `No changes were found to the following directory: ${editDir}`
  }
  const patchKey = applyToAll ? patchedPkgManifest.name : `${patchedPkgManifest.name}@${patchedPkgManifest.version}`
  const patchedDependencies = await savePatch({ lockfileDir, patchKey, patchContent }, opts)

  await updateLockfileSnapshots({
    lockfileDir,
    patchedPkgManifest,
    applyToAll,
    useGitBranchLockfile: opts.useGitBranchLockfile,
    mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
  })

  await install.handler({
    ...opts,
    patchedDependencies,
    frozenLockfile: false,
  })
  return undefined
}

/**
 * The package the edit directory is diffed against: its git-hosted tarball when the lockfile
 * resolved it to one and the patch targets a single version, otherwise its name and version.
 */
async function getPatchedPkgToDiff (
  patchedPkgManifest: PackageManifest,
  opts: GetPatchedDependencyOptions & { applyToAll: boolean }
): Promise<ParseWantedDependencyResult> {
  const nameAndVersion = `${patchedPkgManifest.name}@${patchedPkgManifest.version}`
  if (opts.applyToAll) return parseWantedDependency(nameAndVersion)
  const gitTarballUrl = await getGitTarballUrlFromLockfile({
    alias: patchedPkgManifest.name,
    bareSpecifier: patchedPkgManifest.version || undefined,
  }, {
    lockfileDir: opts.lockfileDir,
    modulesDir: opts.modulesDir,
    virtualStoreDir: opts.virtualStoreDir,
  })
  return parseWantedDependency(gitTarballUrl ? `${patchedPkgManifest.name}@${gitTarballUrl}` : nameAndVersion)
}

async function diffEditDir (editDir: string, patchedPkg: ParseWantedDependencyResult, opts: GetPatchContentOptions): Promise<string> {
  const patchedPkgDir = await preparePkgFilesForDiff(editDir)
  const patchContent = await getPatchContent({
    patchedPkg,
    patchedPkgDir,
    tmpName: createShortHash(editDir),
  }, opts)
  if (patchedPkgDir !== editDir) {
    fs.rmSync(patchedPkgDir, { recursive: true })
  }
  return patchContent
}

/** Writes the patch file and records it in `patchedDependencies`, returning the updated map. */
async function savePatch (
  patch: { lockfileDir: string, patchKey: string, patchContent: string },
  opts: PatchCommitCommandOptions
): Promise<Record<string, string>> {
  const patchedDependencies = {
    ...opts.patchedDependencies,
    [patch.patchKey]: await writePatchFile({ ...patch, patchesDir: opts.patchesDir }),
  }
  await updatePatchedDependencies(patchedDependencies, {
    ...opts,
    workspaceDir: opts.workspaceDir ?? opts.rootProjectManifestDir,
  })
  return patchedDependencies
}

/** Writes the patch into the patches directory and returns its path relative to the lockfile directory. */
async function writePatchFile (opts: {
  lockfileDir: string
  patchesDir: string | undefined
  patchKey: string
  patchContent: string
}): Promise<string> {
  const patchesDirName = normalizePath(path.normalize(opts.patchesDir ?? 'patches'))
  const patchesDir = path.join(opts.lockfileDir, patchesDirName)
  await fs.promises.mkdir(patchesDir, { recursive: true })

  const patchFileName = opts.patchKey.replace('/', '__')
  await fs.promises.writeFile(path.join(patchesDir, `${patchFileName}.patch`), opts.patchContent, 'utf8')
  return `${patchesDirName}/${patchFileName}.patch`
}

interface UpdateLockfileSnapshotsOptions {
  lockfileDir: string
  patchedPkgManifest: PackageManifest
  applyToAll: boolean
  useGitBranchLockfile?: boolean
  mergeGitBranchLockfiles?: boolean
}

async function updateLockfileSnapshots ({
  lockfileDir,
  patchedPkgManifest,
  applyToAll,
  useGitBranchLockfile,
  mergeGitBranchLockfiles,
}: UpdateLockfileSnapshotsOptions): Promise<void> {
  const lockfile = await readWantedLockfile(lockfileDir, {
    ignoreIncompatible: true,
    useGitBranchLockfile,
    mergeGitBranchLockfiles,
  })
  if (!lockfile?.packages) return

  let lockfileChanged = false
  for (const [depPath, snapshot] of Object.entries(lockfile.packages)) {
    const { name, version } = nameVerFromPkgSnapshot(depPath, snapshot)
    const isPatchedSnapshot = name === patchedPkgManifest.name && (applyToAll || version === patchedPkgManifest.version)
    if (isPatchedSnapshot && removeUndeclaredDependencies(snapshot, patchedPkgManifest)) {
      lockfileChanged = true
    }
  }

  if (lockfileChanged) {
    const prunedLockfile = pruneSharedLockfile(lockfile)
    await writeWantedLockfile(lockfileDir, prunedLockfile, {
      useGitBranchLockfile,
      mergeGitBranchLockfiles,
    })
  }
}

type SnapshotDependenciesField = 'dependencies' | 'optionalDependencies' | 'peerDependencies'

/**
 * Drops the snapshot's dependencies that the patched manifest no longer declares, and returns
 * whether any was dropped.
 */
function removeUndeclaredDependencies (snapshot: PackageSnapshot, patchedPkgManifest: PackageManifest): boolean {
  const declaredDeps = {
    ...patchedPkgManifest.peerDependencies,
    ...patchedPkgManifest.optionalDependencies,
    ...patchedPkgManifest.dependencies,
  }
  const removedDeps = removeUndeclaredFromField(snapshot, 'dependencies', declaredDeps)
  const removedOptionalDeps = removeUndeclaredFromField(snapshot, 'optionalDependencies', patchedPkgManifest.optionalDependencies ?? {})
  const removedPeerDeps = removeUndeclaredFromField(snapshot, 'peerDependencies', patchedPkgManifest.peerDependencies ?? {})
  return removedDeps || removedOptionalDeps || removedPeerDeps
}

function removeUndeclaredFromField (
  snapshot: PackageSnapshot,
  field: SnapshotDependenciesField,
  declaredDeps: Record<string, string>
): boolean {
  const deps = snapshot[field]
  if (deps == null) return false
  let removed = false
  for (const depName of Object.keys(deps)) {
    if (Object.prototype.hasOwnProperty.call(declaredDeps, depName)) continue
    delete deps[depName]
    removed = true
  }
  if (Object.keys(deps).length === 0) {
    delete snapshot[field]
  }
  return removed
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
  const { stdout, stderr } = await runGitDiff(folderAN, folderBN)

  if (stderr.length > 0) {
    throw new Error(
      'Unable to diff directories. Make sure you have a recent version of \'git\' available in PATH.\n' +
      `The following error was reported by 'git':\n${stderr}`
    )
  }

  return stdout
    .replace(new RegExp(`(a|b)(${escapeStringRegexp(`/${removeTrailingAndLeadingSlash(folderAN)}/`)})`, 'g'), '$1/')
    .replace(new RegExp(`(a|b)${escapeStringRegexp(`/${removeTrailingAndLeadingSlash(folderBN)}/`)}`, 'g'), '$1/')
    .replace(new RegExp(escapeStringRegexp(`${folderAN}/`), 'g'), '')
    .replace(new RegExp(escapeStringRegexp(`${folderBN}/`), 'g'), '')
    .replace(/\n\\ No newline at end of file\n$/, '\n')
    .replace(/^diff --git a\/.*\.DS_Store b\/.*\.DS_Store[\s\S]+?(?=^diff --git)/gm, '')
    .replace(/^diff --git a\/.*\.DS_Store b\/.*\.DS_Store[\s\S]*$/gm, '')
}

/** Runs `git diff --no-index`, which exits with 1 when the folders differ. */
async function runGitDiff (folderAN: string, folderBN: string): Promise<{ stdout: string, stderr: string }> {
  try {
    const result = await execa('git', ['-c', 'core.safecrlf=false', '-c', 'core.quotePath=false', 'diff', '--src-prefix=a/', '--dst-prefix=b/', '--ignore-cr-at-eol', '--irreversible-delete', '--full-index', '--no-index', '--text', '--no-ext-diff', '--no-color', '--', folderAN, folderBN], {
      cwd: process.cwd(),
      env: {
        ...process.env,
        // #region Predictable output
        // These variables aim to ignore the global git config so we get predictable output
        // https://git-scm.com/docs/git#Documentation/git.txt-codeGITCONFIGNOSYSTEMcode
        GIT_CONFIG_NOSYSTEM: '1',
        // Redirect the global git config to /dev/null instead of setting
        // HOME to an empty string. An empty HOME causes git to resolve '~' as
        // '/' (root), which triggers a "Permission denied" warning when git
        // tries to access '/.config/git/attributes', making pnpm throw an
        // error because any stderr output is treated as a failure.
        // We do not set XDG_CONFIG_HOME to avoid the same issue: an empty
        // value would make git resolve paths like /git/config and /git/attributes.
        // We use '/dev/null' literally instead of os.devNull because on Windows
        // os.devNull is '\\.\nul', which git cannot open as a config file path
        // (fatal: unable to access '\\.\nul': Invalid argument). Git for Windows
        // translates '/dev/null' correctly via its MSYS2 layer.
        GIT_CONFIG_GLOBAL: '/dev/null',
        // #endregion
      },
      stripFinalNewline: false,
    })
    return { stdout: result.stdout as string, stderr: result.stderr as string }
  } catch (err: any) { // eslint-disable-line
    if (err.exitCode !== 1) {
      const errorMessage = (err.stderr as string) || (err.message as string) || ''
      throw new Error(
        'Unable to diff directories. Make sure you have a recent version of \'git\' available in PATH.\n' +
        `The following error was reported:\n${errorMessage}`,
        { cause: err }
      )
    }
    return { stdout: err.stdout as string, stderr: err.stderr as string }
  }
}

function removeTrailingAndLeadingSlash (dirPath: string): string {
  if (dirPath[0] === '/' || dirPath.endsWith('/')) {
    return dirPath.replace(/^\/|\/$/g, '')
  }
  return dirPath
}

/**
 * Link files from the source directory to a new temporary directory,
 * but only if not all files in the source directory should be included in the package.
 * If all files should be included, return the original source directory without creating any links.
 * This is required in order for the diff to not include files that are not part of the package.
 */
export async function preparePkgFilesForDiff (src: string, packageFiles?: string[]): Promise<string> {
  const files = packageFiles ?? Array.from(new Set((await packlist(src)).map((f) => path.join(f))))
  if (await areAllFilesInPkg(files, src)) {
    return src
  }
  const dest = `${src}_tmp`
  await makeEmptyDir(dest)
  await Promise.all(
    files.map(async (file) => {
      const destFile = path.join(dest, file)
      await fs.promises.mkdir(path.dirname(destFile), { recursive: true })
      await linkOrCopyFile(path.join(src, file), destFile)
    })
  )
  return dest
}

async function linkOrCopyFile (srcFile: string, destFile: string): Promise<void> {
  try {
    await fs.promises.link(srcFile, destFile)
  } catch (err: unknown) {
    if (!isUnsupportedLinkError(err)) throw err
    const stat = await fs.promises.lstat(srcFile)
    if (stat.isSymbolicLink()) {
      const target = await fs.promises.readlink(srcFile)
      await fs.promises.symlink(target, destFile)
    } else {
      await fs.promises.copyFile(srcFile, destFile)
    }
  }
}

function isUnsupportedLinkError (err: unknown): boolean {
  return (
    isError(err) &&
    'code' in err &&
    typeof err.code === 'string' &&
    ['EXDEV', 'EPERM', 'EACCES', 'ENOTSUP', 'EOPNOTSUPP'].includes(err.code)
  )
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
