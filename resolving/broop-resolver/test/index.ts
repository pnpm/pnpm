/// <reference path="../../../__typings__/index.d.ts"/>
import { resolveBroop } from '@pnpm/resolving.broop-resolver'
import type { FetchFromRegistry } from '@pnpm/fetching-types'
import type { BinaryResolution, VariationsResolution } from '@pnpm/resolver-base'
import type { PkgResolutionId } from '@pnpm/types'

const RIPGREP_FORMULA = {
  name: 'ripgrep',
  full_name: 'ripgrep',
  aliases: ['rg'],
  versions: {
    stable: '15.1.0',
    bottle: true,
  },
  bottle: {
    stable: {
      root_url: 'https://ghcr.io/v2/homebrew/core',
      files: {
        arm64_sequoia: {
          url: 'https://ghcr.io/v2/homebrew/core/ripgrep/blobs/sha256:0153b06af62b4b8c6ed3f2756dcc4859f74a6128a286f976740468229265cfbe',
          sha256: '0153b06af62b4b8c6ed3f2756dcc4859f74a6128a286f976740468229265cfbe',
        },
        sonoma: {
          url: 'https://ghcr.io/v2/homebrew/core/ripgrep/blobs/sha256:ab382b4ae86aba1b7e6acab3bc50eb64be7bb08cf33a37a32987edb8bc6affe4',
          sha256: 'ab382b4ae86aba1b7e6acab3bc50eb64be7bb08cf33a37a32987edb8bc6affe4',
        },
        x86_64_linux: {
          url: 'https://ghcr.io/v2/homebrew/core/ripgrep/blobs/sha256:349bc55db5ad4b4e8935b889d44c745ae23605c1d57d6eb639dbd5c86d573a88',
          sha256: '349bc55db5ad4b4e8935b889d44c745ae23605c1d57d6eb639dbd5c86d573a88',
        },
      },
    },
  },
  dependencies: ['pcre2'],
  build_dependencies: ['rust'],
}

const GHCR_TOKEN_RESPONSE = { token: 'mock-ghcr-token' }

const SCOOP_RIPGREP = {
  version: '15.1.0',
  architecture: {
    '64bit': {
      url: 'https://github.com/BurntSushi/ripgrep/releases/download/15.1.0/ripgrep-15.1.0-x86_64-pc-windows-msvc.zip',
      hash: '124510b94b6baa3380d051fdf4650eaa80a302c876d611e9dba0b2e18d87493a',
      extract_dir: 'ripgrep-15.1.0-x86_64-pc-windows-msvc',
    },
    arm64: {
      url: 'https://github.com/BurntSushi/ripgrep/releases/download/15.1.0/ripgrep-15.1.0-aarch64-pc-windows-msvc.zip',
      hash: '00d931fb5237c9696ca49308818edb76d8eb6fc132761cb2b1bd616b2df02f8e',
      extract_dir: 'ripgrep-15.1.0-aarch64-pc-windows-msvc',
    },
  },
  bin: 'rg.exe',
}

function createMockFetch (): FetchFromRegistry {
  return (async (url: string, opts?: { redirect?: string }) => {
    // Homebrew formula API
    if (url.includes('formulae.brew.sh/api/formula/ripgrep.json')) {
      return {
        ok: true,
        json: async () => RIPGREP_FORMULA,
        headers: new Headers(),
      }
    }

    // GHCR token endpoint
    if (url.includes('ghcr.io/token')) {
      return {
        ok: true,
        json: async () => GHCR_TOKEN_RESPONSE,
        headers: new Headers(),
      }
    }

    // GHCR blob redirect
    if (url.includes('ghcr.io/v2/homebrew/core') && opts?.redirect === 'manual') {
      const sha = url.split('sha256:')[1]
      return {
        ok: true,
        headers: new Headers({
          location: `https://cdn.example.com/bottles/${sha}`,
        }),
      }
    }

    // Scoop bucket
    if (url.includes('ScoopInstaller') && url.endsWith('ripgrep.json')) {
      return {
        ok: true,
        json: async () => SCOOP_RIPGREP,
        headers: new Headers(),
      }
    }

    // Unknown URL — return 404
    return {
      ok: false,
      status: 404,
      statusText: 'Not Found',
      headers: new Headers(),
    }
  }) as unknown as FetchFromRegistry
}

test('resolveBroop returns null for non-broop specifiers', async () => {
  const result = await resolveBroop(
    { fetchFromRegistry: createMockFetch() },
    { bareSpecifier: 'lodash@4' }
  )
  expect(result).toBeNull()
})

test('resolveBroop resolves ripgrep from Homebrew and Scoop', async () => {
  const result = await resolveBroop(
    { fetchFromRegistry: createMockFetch() },
    { bareSpecifier: 'broop:ripgrep' }
  )

  expect(result).not.toBeNull()
  expect(result!.resolvedVia).toBe('broop')
  expect(result!.alias).toBe('ripgrep')
  expect(result!.manifest?.version).toBe('15.1.0')

  const resolution = result!.resolution as VariationsResolution
  expect(resolution.type).toBe('variations')

  // Should have variants for darwin/arm64, darwin/x64, linux/x64, win32/x64, win32/arm64
  expect(resolution.variants.length).toBeGreaterThanOrEqual(3)

  // Check that Homebrew variants use CDN URLs (resolved from GHCR redirects)
  const darwinVariant = resolution.variants.find(
    (v) => v.targets.some((t) => t.os === 'darwin' && t.cpu === 'arm64')
  )
  expect(darwinVariant).toBeDefined()
  const darwinResolution = darwinVariant!.resolution as BinaryResolution
  expect(darwinResolution.type).toBe('binary')
  expect(darwinResolution.archive).toBe('tarball')
  expect(darwinResolution.url).toMatch(/^https:\/\//)
  expect(darwinResolution.bin).toBe('15.1.0/bin/rg')
  expect(darwinResolution.integrity).toMatch(/^sha256-/)

  // Check that Scoop variants exist for Windows
  const winVariant = resolution.variants.find(
    (v) => v.targets.some((t) => t.os === 'win32' && t.cpu === 'x64')
  )
  expect(winVariant).toBeDefined()
  const winResolution = winVariant!.resolution as BinaryResolution
  expect(winResolution.type).toBe('binary')
  expect(winResolution.archive).toBe('zip')
  expect(winResolution.bin).toBe('rg.exe')
  expect(winResolution.prefix).toBe('ripgrep-15.1.0-x86_64-pc-windows-msvc')
})

test('resolveBroop uses cached resolution when available and not updating', async () => {
  const cachedResolution: VariationsResolution = {
    type: 'variations',
    variants: [],
  }

  const result = await resolveBroop(
    { fetchFromRegistry: createMockFetch() },
    { bareSpecifier: 'broop:ripgrep' },
    {
      currentPkg: {
        id: 'ripgrep@broop:15.0.0' as PkgResolutionId,
        resolution: cachedResolution,
      },
      lockfileDir: '/tmp',
      projectDir: '/tmp',
      preferredVersions: {},
    }
  )

  expect(result).not.toBeNull()
  expect(result!.resolution).toBe(cachedResolution)
})

test('resolveBroop parses version from specifier', async () => {
  const result = await resolveBroop(
    { fetchFromRegistry: createMockFetch() },
    { bareSpecifier: 'broop:ripgrep@15.1.0' }
  )

  expect(result).not.toBeNull()
  expect(result!.id).toBe('ripgrep@broop:15.1.0')
})

test('resolveBroop includes transitive dependencies as broop: specifiers', async () => {
  const result = await resolveBroop(
    { fetchFromRegistry: createMockFetch() },
    { bareSpecifier: 'broop:ripgrep' }
  )

  expect(result).not.toBeNull()
  // Homebrew's ripgrep depends on pcre2
  expect(result!.manifest?.optionalDependencies).toEqual({
    pcre2: 'broop:pcre2',
  })
})

test('resolveBroop throws in offline mode', async () => {
  await expect(
    resolveBroop(
      { fetchFromRegistry: createMockFetch(), offline: true },
      { bareSpecifier: 'broop:ripgrep' }
    )
  ).rejects.toThrow('Cannot resolve broop packages in offline mode')
})
