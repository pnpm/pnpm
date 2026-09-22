/// <reference path="../../../__typings__/index.d.ts"/>
import fs from 'node:fs'
import path from 'node:path'
import { pathToFileURL } from 'node:url'

import { afterAll, beforeEach, expect, jest, test } from '@jest/globals'
import type { PnpmError } from '@pnpm/error'
import { createCafsStore } from '@pnpm/store.create-cafs-store'
import { StoreIndex } from '@pnpm/store.index'
import { lexCompare } from '@pnpm/text.ordinal-comparator'
import { temporaryDirectory } from 'tempy'

const realExeca = (await import('execa')).safeExeca
{
  const originalModule = await import('execa')
  jest.unstable_mockModule('execa', () => {
    return {
      __esModule: true,
      ...originalModule,
      safeExeca: jest.fn(realExeca),
    }
  })
}
{
  const originalModule = await import('@pnpm/logger')
  jest.unstable_mockModule('@pnpm/logger', () => {
    return {
      ...originalModule,
      globalWarn: jest.fn(),
    }
  })
}

const { globalWarn } = await import('@pnpm/logger')
const { safeExeca: execa } = await import('execa')
const { createGitFetcher } = await import('@pnpm/fetching.git-fetcher')

const storeIndexes: StoreIndex[] = []
afterAll(() => {
  for (const si of storeIndexes) si.close()
})

function createStoreIndex (storeDir: string): StoreIndex {
  const si = new StoreIndex(storeDir)
  storeIndexes.push(si)
  return si
}

beforeEach(() => {
  jest.mocked(execa).mockReset()
  jest.mocked(execa).mockImplementation(realExeca)
  jest.mocked(globalWarn).mockClear()
})

test('uses short CAFS temp directories for git package preparation', async () => {
  const storeDir = temporaryDirectory()
  const tmpDir = await createCafsStore(storeDir).tempDir()

  expect(path.dirname(tmpDir)).toBe(path.join(storeDir, 'tmp'))
  expect(path.basename(tmpDir)).toMatch(/^_tmp_[a-zA-Z0-9]{6}$/)
  expect(path.basename(tmpDir).length).toBeLessThanOrEqual(11)
})

test('fetch', async () => {
  const storeDir = temporaryDirectory()
  const fetch = createGitFetcher({ storeIndex: createStoreIndex(storeDir) }).git
  const { filesMap, manifest } = await fetch(
    createCafsStore(storeDir),
    {
      commit: 'c9b30e71d704cd30fa71f2edd1ecc7dcc4985493',
      repo: 'https://github.com/kevva/is-positive.git',
      type: 'git',
    },
    {
      readManifest: true,
      filesIndexFile: path.join(storeDir, 'index.json'),
    }
  )
  expect(filesMap.has('package.json')).toBeTruthy()
  expect(manifest?.name).toBe('is-positive')
})

test('fetch includes committed Git submodules', async () => {
  const root = temporaryDirectory()
  const moduleDir = path.join(root, 'module')
  const packageDir = path.join(root, 'package')
  await Promise.all([moduleDir, packageDir].map(async (directory) => {
    fs.mkdirSync(directory)
    await execa('git', ['init', '-q', '-b', 'main'], { cwd: directory })
    await execa('git', ['config', 'user.email', 'test@example.invalid'], { cwd: directory })
    await execa('git', ['config', 'user.name', 'Test'], { cwd: directory })
  }))
  fs.writeFileSync(path.join(moduleDir, 'answer.js'), 'module.exports = 42\n')
  await commitAll(moduleDir, 'add module')
  fs.writeFileSync(path.join(packageDir, 'package.json'), '{"name":"with-submodule","version":"1.0.0"}')
  await commitAll(packageDir, 'initialize package')
  await execa('git', [
    '-c', 'protocol.file.allow=always',
    'submodule', 'add', '--', pathToFileURL(moduleDir).href, 'native',
  ], { cwd: packageDir })
  await commitAll(packageDir, 'add module')
  const { stdout: commit } = await execa('git', ['rev-parse', 'HEAD'], { cwd: packageDir })
  const storeDir = temporaryDirectory()
  const fetch = createGitFetcher({ storeIndex: createStoreIndex(storeDir) }).git

  const { filesMap } = await withEnv({
    GIT_CONFIG_COUNT: '1',
    GIT_CONFIG_KEY_0: 'protocol.file.allow',
    GIT_CONFIG_VALUE_0: 'always',
  }, async () => fetch(
    createCafsStore(storeDir),
    { commit: String(commit).trim(), repo: pathToFileURL(packageDir).href, type: 'git' },
    { filesIndexFile: path.join(storeDir, 'index.json') }
  ))

  expect(filesMap.has('native/answer.js')).toBeTruthy()
})

test('fetch a package from Git sub folder', async () => {
  const storeDir = temporaryDirectory()
  const fetch = createGitFetcher({ storeIndex: createStoreIndex(storeDir) }).git
  const { filesMap } = await fetch(
    createCafsStore(storeDir),
    {
      commit: 'e2ad6effb15541c76f39884e5231464ff383853b',
      repo: 'https://github.com/RexSkz/test-git-subfolder-fetch.git',
      path: '/packages/simple-react-app',
      type: 'git',
    },
    {
      filesIndexFile: path.join(storeDir, 'index.json'),
    }
  )
  expect(filesMap.has('public/index.html')).toBeTruthy()
})

test('prevent directory traversal attack when using Git sub folder', async () => {
  const storeDir = temporaryDirectory()
  const fetch = createGitFetcher({ storeIndex: createStoreIndex(storeDir) }).git
  const repo = 'https://github.com/RexSkz/test-git-subfolder-fetch.git'
  const pkgDir = '../../etc'
  await expect(
    fetch(
      createCafsStore(storeDir),
      {
        commit: 'e2ad6effb15541c76f39884e5231464ff383853b',
        repo,
        path: pkgDir,
        type: 'git',
      },
      {
        filesIndexFile: path.join(storeDir, 'index.json'),
      }
    )
  ).rejects.toThrow(`Failed to prepare git-hosted package fetched from "${repo}": Path "${pkgDir}" should be a sub directory`)
})

test('prevent directory traversal attack when using Git sub folder #2', async () => {
  const storeDir = temporaryDirectory()
  const fetch = createGitFetcher({ storeIndex: createStoreIndex(storeDir) }).git
  const repo = 'https://github.com/RexSkz/test-git-subfolder-fetch.git'
  const pkgDir = 'not/exists'
  await expect(
    fetch(
      createCafsStore(storeDir),
      {
        commit: 'e2ad6effb15541c76f39884e5231464ff383853b',
        repo,
        path: pkgDir,
        type: 'git',
      },
      {
        filesIndexFile: path.join(storeDir, 'index.json'),
      }
    )
  ).rejects.toThrow(`Failed to prepare git-hosted package fetched from "${repo}": Path "${pkgDir}" is not a directory`)
})

test('fetch a package from Git that has a prepare script', async () => {
  const storeDir = temporaryDirectory()
  const fetch = createGitFetcher({
    storeIndex: createStoreIndex(storeDir),
  }).git
  const pkgResolutionId = 'git+https://github.com/pnpm/test-git-fetch.git#8b333f12d5357f4f25a654c305c826294cb073bf&path:packages/test-git-fetch'
  const { filesMap, requiresPrepare } = await fetch(
    createCafsStore(storeDir),
    {
      commit: '8b333f12d5357f4f25a654c305c826294cb073bf',
      repo: 'https://github.com/pnpm/test-git-fetch.git',
      type: 'git',
    },
    {
      allowBuild: (depPath) => depPath === `test-git-fetch@${pkgResolutionId}`,
      filesIndexFile: path.join(storeDir, 'index.json'),
      pkgResolutionId,
    }
  )
  expect(filesMap.has('dist/index.js')).toBeTruthy()
  expect(requiresPrepare).toBe(true)
})

// Test case for https://github.com/pnpm/pnpm/issues/1866
test('fetch a package without a package.json', async () => {
  const storeDir = temporaryDirectory()
  const fetch = createGitFetcher({ storeIndex: createStoreIndex(storeDir) }).git
  const { filesMap } = await fetch(
    createCafsStore(storeDir),
    {
      // a small Deno library with a 'denolib.json' instead of a 'package.json'
      commit: 'aeb6b15f9c9957c8fa56f9731e914c4d8a6d2f2b',
      repo: 'https://github.com/denolib/camelcase.git',
      type: 'git',
    },
    {
      filesIndexFile: path.join(storeDir, 'index.json'),
    }
  )
  expect(filesMap.has('denolib.json')).toBeTruthy()
})

// Covers the regression reported in https://github.com/pnpm/pnpm/issues/4064
test('fetch a big repository', async () => {
  const storeDir = temporaryDirectory()
  const fetch = createGitFetcher({ storeIndex: createStoreIndex(storeDir) }).git
  const { filesMap } = await fetch(createCafsStore(storeDir),
    {
      commit: 'f766801580f10543c24ba8bfa59046a776848097',
      repo: 'https://github.com/pnpm-e2e/drupal-js-build.git',
      type: 'git',
    }, {
      filesIndexFile: path.join(storeDir, 'index.json'),
    })
  expect(filesMap).toBeTruthy()
})

test('still able to shallow fetch for allowed hosts', async () => {
  const storeDir = temporaryDirectory()
  const fetch = createGitFetcher({ gitShallowHosts: ['github.com'], storeIndex: createStoreIndex(storeDir) }).git
  const resolution = {
    commit: 'c9b30e71d704cd30fa71f2edd1ecc7dcc4985493',
    repo: 'https://github.com/kevva/is-positive.git',
    type: 'git' as const,
  }
  const { filesMap, manifest } = await fetch(createCafsStore(storeDir), resolution, {
    readManifest: true,
    filesIndexFile: path.join(storeDir, 'index.json'),
  })
  const calls = gitCalls()
  const expectedCalls = [
    ['git', [...prefixGitArgs(), 'init']],
    ['git', [...prefixGitArgs(), 'remote', 'add', 'origin', resolution.repo]],
    [
      'git',
      [...prefixGitArgs(), 'fetch', '--depth', '1', 'origin', resolution.commit],
    ],
  ]
  for (let i = 1; i < expectedCalls.length; i++) {
    // Discard final argument as it passes temporary directory
    expect(calls[i].slice(0, -1)).toEqual(expectedCalls[i])
  }
  expect(filesMap.has('package.json')).toBeTruthy()
  expect(manifest?.name).toBe('is-positive')
})

test('fail when preparing a git-hosted package', async () => {
  const storeDir = temporaryDirectory()
  const fetch = createGitFetcher({
    storeIndex: createStoreIndex(storeDir),
  }).git
  await expect(
    fetch(createCafsStore(storeDir),
      {
        commit: 'ba58874aae1210a777eb309dd01a9fdacc7e54e7',
        repo: 'https://github.com/pnpm-e2e/prepare-script-fails.git',
        type: 'git',
      }, {
        allowBuild: (depPath) => depPath.startsWith('@pnpm.e2e/prepare-script-fails@'),
        filesIndexFile: path.join(storeDir, 'index.json'),
      })
  ).rejects.toThrow('Failed to prepare git-hosted package fetched from "https://github.com/pnpm-e2e/prepare-script-fails.git": @pnpm.e2e/prepare-script-fails@1.0.0 npm-install: `npm install`')
})

test('reject a partial commit before invoking git', async () => {
  const storeDir = temporaryDirectory()
  const fetch = createGitFetcher({
    storeIndex: createStoreIndex(storeDir),
  }).git
  await expect(
    fetch(createCafsStore(storeDir),
      {
        commit: 'deadbeef',
        repo: 'https://github.com/pnpm-e2e/simple-pkg.git',
        type: 'git',
      }, {
        filesIndexFile: path.join(storeDir, 'index.json'),
      })
  ).rejects.toThrow('Invalid git commit hash "deadbeef"')
  expect(jest.mocked(execa)).not.toHaveBeenCalled()
})

test('reject a commit value that looks like a git option', async () => {
  const storeDir = temporaryDirectory()
  const fetch = createGitFetcher({ storeIndex: createStoreIndex(storeDir) }).git
  await expect(
    fetch(createCafsStore(storeDir),
      {
        commit: '--upload-pack=touch /tmp/pwned',
        repo: 'file:///tmp/repo.git',
        type: 'git',
      }, {
        filesIndexFile: path.join(storeDir, 'index.json'),
      })
  ).rejects.toThrow('Invalid git commit hash "--upload-pack=touch /tmp/pwned"')
  expect(jest.mocked(execa)).not.toHaveBeenCalled()
})

test('do not build the package when scripts are ignored', async () => {
  const storeDir = temporaryDirectory()
  const fetch = createGitFetcher({ ignoreScripts: true, storeIndex: createStoreIndex(storeDir) }).git
  const { filesMap } = await fetch(createCafsStore(storeDir),
    {
      commit: '55416a9c468806a935636c0ad0371a14a64df8c9',
      repo: 'https://github.com/pnpm-e2e/prepare-script-works.git',
      type: 'git',
    }, {
      filesIndexFile: path.join(storeDir, 'index.json'),
    })
  expect(filesMap.has('package.json')).toBeTruthy()
  expect(filesMap.has('prepare.txt')).toBeFalsy()
  expect(globalWarn).toHaveBeenCalledWith('The git-hosted package fetched from "https://github.com/pnpm-e2e/prepare-script-works.git" has to be built but the build scripts were ignored.')
})

test('block git package with prepare script', async () => {
  const storeDir = temporaryDirectory()
  const fetch = createGitFetcher({ storeIndex: createStoreIndex(storeDir) }).git
  const repo = 'https://github.com/pnpm-e2e/prepare-script-works.git'
  await expect(
    fetch(createCafsStore(storeDir),
      {
        commit: '55416a9c468806a935636c0ad0371a14a64df8c9',
        repo,
        type: 'git',
      }, {
        allowBuild: () => undefined,
        filesIndexFile: path.join(storeDir, 'index.json'),
      })
  ).rejects.toThrow('The git-hosted package "@pnpm.e2e/prepare-script-works@1.0.0" needs to execute build scripts but is not in the "allowBuilds" allowlist')
})

test('allow git package with prepare script', async () => {
  const storeDir = temporaryDirectory()
  const fetch = createGitFetcher({
    storeIndex: createStoreIndex(storeDir),
  }).git
  // This should succeed without throwing because the package is in the allowlist
  const { filesMap } = await fetch(createCafsStore(storeDir),
    {
      commit: '55416a9c468806a935636c0ad0371a14a64df8c9',
      repo: 'https://github.com/pnpm-e2e/prepare-script-works.git',
      type: 'git',
    }, {
      allowBuild: (depPath) => depPath.startsWith('@pnpm.e2e/prepare-script-works@'),
      filesIndexFile: path.join(storeDir, 'index.json'),
    })
  expect(filesMap.has('package.json')).toBeTruthy()
  // Note: prepare.txt is in .gitignore so it won't be in the files index
  // The fact that no error was thrown proves the prepare script was allowed to run
})

function prefixGitArgs (): string[] {
  return process.platform === 'win32' ? ['-c', 'core.longpaths=true'] : []
}

test('fetch only the included files', async () => {
  const storeDir = temporaryDirectory()
  const fetch = createGitFetcher({ storeIndex: createStoreIndex(storeDir) }).git
  const { filesMap } = await fetch(
    createCafsStore(storeDir),
    {
      commit: '958d6d487217512bb154d02836e9b5b922a600d8',
      repo: 'https://github.com/pnpm-e2e/pkg-with-ignored-files',
      type: 'git',
    },
    {
      filesIndexFile: path.join(storeDir, 'index.json'),
    }
  )
  expect(Array.from(filesMap.keys()).sort(lexCompare)).toStrictEqual([
    'README.md',
    'dist/index.js',
    'package.json',
  ])
})

// Covers https://github.com/pnpm/pnpm/issues/13743.
test('a failed clone over SSH names the package and how to re-record it', async () => {
  const storeDir = temporaryDirectory()
  const fetch = createGitFetcher({ storeIndex: createStoreIndex(storeDir) }).git
  const err = await fetchFailure(withoutSsh(async () => fetch(
    createCafsStore(storeDir),
    {
      commit: 'c9b30e71d704cd30fa71f2edd1ecc7dcc4985493',
      repo: 'git@github.com:pnpm-e2e/this-repository-does-not-exist.git',
      type: 'git',
    },
    {
      filesIndexFile: path.join(storeDir, 'index.json'),
      pkg: { name: '@scope/pkg', version: '1.0.0' },
    }
  )))

  expect(err.code).toBe('ERR_PNPM_GIT_FETCH_FAILED')
  expect(err.message).toContain('Failed to fetch "@scope/pkg" from the git repository "git@github.com:pnpm-e2e/this-repository-does-not-exist.git"')
  expect(err.hint).toContain('needs an SSH key for github.com')
  expect(err.hint).toContain('pnpm update @scope/pkg')
})

test('a failed clone over HTTPS carries no SSH remediation', async () => {
  const storeDir = temporaryDirectory()
  const fetch = createGitFetcher({ storeIndex: createStoreIndex(storeDir) }).git
  const err = await fetchFailure(fetch(
    createCafsStore(storeDir),
    {
      commit: 'c9b30e71d704cd30fa71f2edd1ecc7dcc4985493',
      repo: 'https://github.com/pnpm-e2e/this-repository-does-not-exist.git',
      type: 'git',
    },
    {
      filesIndexFile: path.join(storeDir, 'index.json'),
      pkg: { name: '@scope/pkg', version: '1.0.0' },
    }
  ))

  expect(err.code).toBe('ERR_PNPM_GIT_FETCH_FAILED')
  expect(err.hint).toBeUndefined()
})

test('a failed shallow fetch is reported like a failed clone', async () => {
  const storeDir = temporaryDirectory()
  const fetch = createGitFetcher({ gitShallowHosts: ['github.com'], storeIndex: createStoreIndex(storeDir) }).git
  const err = await fetchFailure(withoutSsh(async () => fetch(
    createCafsStore(storeDir),
    {
      commit: 'c9b30e71d704cd30fa71f2edd1ecc7dcc4985493',
      repo: 'ssh://git@github.com/pnpm-e2e/this-repository-does-not-exist.git',
      type: 'git',
    },
    {
      filesIndexFile: path.join(storeDir, 'index.json'),
      pkg: { name: '@scope/pkg', version: '1.0.0' },
    }
  )))

  expect(jest.mocked(execa).mock.calls.some(([, args]) => (args as string[])?.includes('fetch'))).toBe(true)
  expect(err.code).toBe('ERR_PNPM_GIT_FETCH_FAILED')
  expect(err.message).toContain('Failed to fetch "@scope/pkg" from the git repository')
  expect(err.hint).toContain('needs an SSH key for github.com')
})

// An IPv6 literal keeps its brackets, matching what pacquet's ssh_repo_host
// returns for the same reference.
test('the SSH remediation names a bracketed IPv6 host', async () => {
  const storeDir = temporaryDirectory()
  const fetch = createGitFetcher({ storeIndex: createStoreIndex(storeDir) }).git
  const err = await fetchFailure(withoutSsh(async () => fetch(
    createCafsStore(storeDir),
    {
      commit: 'c9b30e71d704cd30fa71f2edd1ecc7dcc4985493',
      repo: 'ssh://git@[2001:db8::1]:2222/org/repo.git',
      type: 'git',
    },
    {
      filesIndexFile: path.join(storeDir, 'index.json'),
      pkg: { name: '@scope/pkg', version: '1.0.0' },
    }
  )))

  expect(err.hint).toContain('needs an SSH key for [2001:db8::1]')
})

test('credentials in the repository URL are redacted from the failure', async () => {
  const storeDir = temporaryDirectory()
  const fetch = createGitFetcher({ storeIndex: createStoreIndex(storeDir) }).git
  const err = await fetchFailure(fetch(
    createCafsStore(storeDir),
    {
      commit: 'c9b30e71d704cd30fa71f2edd1ecc7dcc4985493',
      repo: 'https://s3cr3t-t0ken:x-oauth-basic@github.com/pnpm-e2e/this-repository-does-not-exist.git',
      type: 'git',
    },
    {
      filesIndexFile: path.join(storeDir, 'index.json'),
      pkg: { name: '@scope/pkg', version: '1.0.0' },
    }
  ))

  expect(err.message).not.toContain('s3cr3t-t0ken')
  expect(err.message).toContain('https://github.com/pnpm-e2e/this-repository-does-not-exist.git')
})

test('git runs with terminal and ssh prompts disabled, so a passphrase prompt cannot block the fetch', async () => {
  const storeDir = temporaryDirectory()
  const fetch = createGitFetcher({ storeIndex: createStoreIndex(storeDir) }).git
  failGit(Object.assign(new Error('git clone failed'), { stderr: 'git@github.com: Permission denied (publickey).' }))
  await fetchFailure(withEnv({ GIT_SSH: undefined, GIT_SSH_COMMAND: undefined }, async () => fetch(
    createCafsStore(storeDir),
    {
      commit: 'c9b30e71d704cd30fa71f2edd1ecc7dcc4985493',
      repo: 'git@github.com:acme/widget.git',
      type: 'git',
    },
    {
      filesIndexFile: path.join(storeDir, 'index.json'),
    }
  )))

  expect(jest.mocked(execa)).toHaveBeenCalledWith(
    'git',
    [...prefixGitArgs(), 'clone', 'git@github.com:acme/widget.git', expect.any(String)],
    expect.objectContaining({
      env: expect.objectContaining({
        GIT_TERMINAL_PROMPT: '0',
        GIT_SSH_COMMAND: 'ssh -o BatchMode=yes',
      }),
    })
  )
})

test('a missing git executable is reported as such, not as a fetch failure', async () => {
  const storeDir = temporaryDirectory()
  const fetch = createGitFetcher({ storeIndex: createStoreIndex(storeDir) }).git
  failGit(Object.assign(new Error('spawn git ENOENT'), { code: 'ENOENT' }))
  const err = await fetchFailure(fetch(
    createCafsStore(storeDir),
    {
      commit: 'c9b30e71d704cd30fa71f2edd1ecc7dcc4985493',
      repo: 'git@github.com:acme/widget.git',
      type: 'git',
    },
    {
      filesIndexFile: path.join(storeDir, 'index.json'),
      pkg: { name: '@scope/pkg', version: '1.0.0' },
    }
  ))

  expect(err.code).toBe('ERR_PNPM_GIT_FETCHER_GIT_NOT_FOUND')
  expect(err.hint).toBeUndefined()
})

/**
 * The git invocations of the fetcher under test, without the `git config`
 * lookup that decides whether ssh runs in batch mode.
 */
function gitCalls (): Array<[string, readonly string[] | undefined, unknown]> {
  return jest.mocked(execa).mock.calls
    .filter(([, args]) => args?.[0] !== 'config') as Array<[string, readonly string[] | undefined, unknown]>
}

/**
 * Makes every git invocation of the fetcher fail with `err`. The `git config`
 * lookup keeps answering that no ssh command is configured.
 */
function failGit (err: Error): void {
  jest.mocked(execa).mockImplementation(((_file: string, args?: readonly string[]) => {
    if (args?.[0] === 'config') return Promise.reject(new Error('core.sshCommand is not configured'))
    throw err
  }) as any) // eslint-disable-line @typescript-eslint/no-explicit-any
}

async function fetchFailure (fetching: Promise<unknown>): Promise<PnpmError> {
  return fetching.then(
    () => {
      throw new Error('expected the git fetch to fail')
    },
    (err: unknown) => err as PnpmError
  )
}

async function withoutSsh<T> (fn: () => Promise<T>): Promise<T> {
  return withEnv({ GIT_SSH_COMMAND: 'false' }, fn)
}

async function withEnv<T> (vars: Record<string, string | undefined>, fn: () => Promise<T>): Promise<T> {
  const original = Object.fromEntries(Object.keys(vars).map((name) => [name, process.env[name]]))
  setEnv(vars)
  try {
    return await fn()
  } finally {
    setEnv(original)
  }
}

function setEnv (vars: Record<string, string | undefined>): void {
  for (const [name, value] of Object.entries(vars)) {
    if (value === undefined) {
      delete process.env[name]
    } else {
      process.env[name] = value
    }
  }
}

async function commitAll (directory: string, message: string): Promise<void> {
  await execa('git', ['add', '-A'], { cwd: directory })
  await execa('git', ['-c', 'commit.gpgsign=false', 'commit', '-q', '-m', message], { cwd: directory })
}
