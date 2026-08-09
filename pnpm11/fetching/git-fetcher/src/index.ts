import assert from 'node:assert'
import fs from 'node:fs/promises'
import path from 'node:path'
import { URL } from 'node:url'
import util from 'node:util'

import { PnpmError } from '@pnpm/error'
import { preparePackage } from '@pnpm/exec.prepare-package'
import type { GitFetcher } from '@pnpm/fetching.fetcher-base'
import { packlist } from '@pnpm/fs.packlist'
import { globalWarn } from '@pnpm/logger'
import { createGitHostedPkgId, sshRepoUrlToHttps } from '@pnpm/resolving.git-resolver'
import type { StoreIndex } from '@pnpm/store.index'
import { addFilesFromDir } from '@pnpm/worker'
import { rimraf } from '@zkochan/rimraf'
import { safeExeca as execa } from 'execa'

export interface CreateGitFetcherOptions {
  gitShallowHosts?: string[]
  storeIndex: StoreIndex
  unsafePerm?: boolean
  userAgent?: string
  ignoreScripts?: boolean
}

export function createGitFetcher (createOpts: CreateGitFetcherOptions): { git: GitFetcher } {
  const allowedHosts = new Set(createOpts?.gitShallowHosts ?? [])
  const ignoreScripts = createOpts.ignoreScripts ?? false

  const gitFetcher: GitFetcher = async (cafs, resolution, opts) => {
    if (!isValidCommitHash(resolution.commit)) {
      throw new PnpmError('INVALID_GIT_COMMIT', `Invalid git commit hash "${resolution.commit}" for repository "${resolution.repo}". Expected a 40-character hexadecimal SHA.`)
    }
    const tempLocation = await cafs.tempDir()
    await downloadRepo(resolution.repo, resolution.commit, tempLocation)
    await execGit(['checkout', resolution.commit], { cwd: tempLocation })
    const receivedCommit = await execGit(['rev-parse', 'HEAD'], { cwd: tempLocation })
    if (receivedCommit.trim() !== resolution.commit) {
      throw new PnpmError('GIT_CHECKOUT_FAILED', `received commit ${receivedCommit.trim()} does not match expected value ${resolution.commit}`)
    }
    let pkgDir: string
    try {
      const prepareResult = await preparePackage({
        allowBuild: opts.allowBuild,
        ignoreScripts: createOpts.ignoreScripts,
        pkgResolutionId: createGitHostedPkgId(resolution),
        unsafePerm: createOpts.unsafePerm,
        userAgent: createOpts.userAgent,
      }, tempLocation, resolution.path ?? '')
      pkgDir = prepareResult.pkgDir
      if (ignoreScripts && prepareResult.shouldBeBuilt) {
        globalWarn(`The git-hosted package fetched from "${resolution.repo}" has to be built but the build scripts were ignored.`)
      }
    } catch (err: unknown) {
      assert(util.types.isNativeError(err))
      err.message = `Failed to prepare git-hosted package fetched from "${resolution.repo}": ${err.message}`
      throw err
    }
    // removing /.git to make directory integrity calculation faster
    await rimraf(path.join(tempLocation, '.git'))
    const files = await packlist(pkgDir)
    // Important! We cannot remove the temp location at this stage.
    // Even though we have the index of the package,
    // the linking of files to the store is in progress.
    return addFilesFromDir({
      storeDir: cafs.storeDir,
      storeIndex: createOpts.storeIndex,
      dir: pkgDir,
      files,
      filesIndexFile: opts.filesIndexFile,
      readManifest: opts.readManifest,
      pkg: opts.pkg,
    })
  }

  async function downloadRepo (repo: string, commit: string, dest: string): Promise<void> {
    try {
      await cloneOrFetch(repo, commit, dest)
      return
    } catch (err: unknown) {
      const httpsRepo = sshRepoUrlToHttps(repo)
      if (httpsRepo == null) throw err
      // A lockfile may record an SSH URL for a dependency whose specifier never
      // asked for SSH, which makes the lockfile unusable wherever no SSH key is
      // configured (CI runners, most commonly). The commit is pinned and
      // verified after checkout either way, so the same repository is retried
      // over HTTPS before the install is failed.
      await rimraf(dest)
      await fs.mkdir(dest, { recursive: true })
      try {
        await cloneOrFetch(httpsRepo, commit, dest)
      } catch {
        throw err
      }
      globalWarn(`Failed to fetch "${repo}" over SSH, so it was fetched from "${httpsRepo}" instead. Re-resolve this dependency to record the HTTPS URL in the lockfile.`)
    }
  }

  async function cloneOrFetch (repo: string, commit: string, dest: string): Promise<void> {
    if (allowedHosts.size > 0 && shouldUseShallow(repo, allowedHosts)) {
      await execGit(['init'], { cwd: dest })
      await execGit(['remote', 'add', 'origin', repo], { cwd: dest })
      await execGit(['fetch', '--depth', '1', 'origin', commit], { cwd: dest })
    } else {
      await execGit(['clone', repo, dest])
    }
  }

  return {
    git: gitFetcher,
  }
}

function isValidCommitHash (commit: string): boolean {
  return /^[0-9a-f]{40}$/i.test(commit)
}

function shouldUseShallow (repoUrl: string, allowedHosts: Set<string>): boolean {
  try {
    const { host } = new URL(repoUrl)
    if (allowedHosts.has(host)) {
      return true
    }
  } catch {
    // URL might be malformed
  }
  return false
}

function prefixGitArgs (): string[] {
  return process.platform === 'win32' ? ['-c', 'core.longpaths=true'] : []
}

async function execGit (args: string[], opts?: object): Promise<string> {
  const fullArgs = prefixGitArgs().concat(args || [])
  const { stdout } = await execa('git', fullArgs, opts)
  return stdout as string
}
