import assert from 'node:assert'
import path from 'node:path'
import { URL } from 'node:url'
import util from 'node:util'

import { PnpmError, redactAndSanitize, redactAndSanitizeMultiline } from '@pnpm/error'
import { preparePackage } from '@pnpm/exec.prepare-package'
import type { GitFetcher } from '@pnpm/fetching.fetcher-base'
import { packlist } from '@pnpm/fs.packlist'
import { globalWarn } from '@pnpm/logger'
import { createGitHostedPkgId } from '@pnpm/resolving.git-resolver'
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
    try {
      if (allowedHosts.size > 0 && shouldUseShallow(resolution.repo, allowedHosts)) {
        await execGit(['init'], { cwd: tempLocation })
        await execGit(['remote', 'add', 'origin', resolution.repo], { cwd: tempLocation })
        await execGit(['fetch', '--depth', '1', 'origin', resolution.commit], { cwd: tempLocation })
      } else {
        await execGit(['clone', resolution.repo, tempLocation])
      }
    } catch (err: unknown) {
      assert(util.types.isNativeError(err))
      throw gitFetchError(err, resolution.repo, opts.pkg?.name)
    }
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

  return {
    git: gitFetcher,
  }
}

function isValidCommitHash (commit: string): boolean {
  return /^[0-9a-f]{40}$/i.test(commit)
}

/**
 * Restate a failure of the transport-touching git invocations, naming the
 * package the resolution belongs to.
 *
 * Every interpolated value is untrusted: a lockfile URL can carry `user:pass@`
 * credentials, and git echoes it back through stderr. Only the values go
 * through {@link redactAndSanitize} — it strips control characters, which would
 * collapse the deliberately multi-line hint.
 */
function gitFetchError (err: Error, repo: string, pkgName?: string): PnpmError {
  if ((err as { code?: string }).code === 'ENOENT') {
    return new PnpmError('GIT_FETCHER_GIT_NOT_FOUND', '`git` executable not found on PATH. Install git to fetch git-hosted packages.')
  }
  const safePkgName = pkgName == null ? undefined : redactAndSanitize(pkgName)
  const subject = safePkgName == null ? '' : `"${safePkgName}" `
  return new PnpmError(
    'GIT_FETCH_FAILED',
    `Failed to fetch ${subject}from the git repository "${redactAndSanitize(repo)}": ${redactAndSanitizeMultiline(gitFailureDetail(err))}`,
    { hint: sshRemediationHint(repo, safePkgName) }
  )
}

// git appends the child's stderr to its own message, which repeats the repository
// and leaks the store's temp directory. The stderr alone is what the user needs.
function gitFailureDetail (err: Error): string {
  const stderr = (err as { stderr?: string }).stderr?.trim()
  return stderr == null || stderr === '' ? err.message : stderr
}

/**
 * Guidance for a git dependency locked to an SSH remote, or `undefined` when the
 * lockfile records a transport that needs no key.
 *
 * A lockfile written before pnpm v11.21 could record an SSH URL for a dependency
 * whose specifier never asked for SSH, and resolution is skipped while that
 * lockfile stays up to date — so the entry survives the upgrade that fixed it and
 * the install keeps failing wherever no SSH key is configured.
 */
function sshRemediationHint (repo: string, pkgName?: string): string | undefined {
  const host = sshRepoHost(repo)
  if (host == null) return undefined
  return `The lockfile records an SSH remote for this dependency, so fetching it needs an SSH key for ${redactAndSanitize(host)}.

If its specifier does not ask for SSH (for example "github:owner/repo"), the lockfile entry was written before pnpm v11.21 and can be re-recorded over HTTPS:

    pnpm update ${pkgName ?? '<package>'}

"pnpm install --force" and "pnpm install --resolution-only" do not re-resolve git dependencies, so neither clears it.`
}

/**
 * The host an SSH git reference points at, or `undefined` if `repo` is not one.
 *
 * Covers the URL form (`[git+]ssh://[user@]host[:port]/path`) and the scp-style
 * shorthand (`[user@]host:path`) that carries no scheme. The `user@` is mandatory
 * in the shorthand, which is what keeps a Windows drive path (`C:\repo`) from
 * being read as a host.
 */
function sshRepoHost (repo: string): string | undefined {
  const sshUrl = repo.replace(/^git\+/, '')
  if (sshUrl.startsWith('ssh://')) {
    try {
      return new URL(sshUrl).hostname || undefined
    } catch {
      return undefined
    }
  }
  if (repo.includes('://')) return undefined
  const colonPos = repo.indexOf(':')
  if (colonPos === -1) return undefined
  const authority = repo.slice(0, colonPos)
  const atPos = authority.lastIndexOf('@')
  return atPos === -1 ? undefined : authority.slice(atPos + 1) || undefined
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
