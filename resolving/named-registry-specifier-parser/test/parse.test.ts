import { describe, expect, test } from '@jest/globals'
import {
  type NamedRegistrySpec,
  parseNamedRegistrySpecifier,
} from '@pnpm/resolving.named-registry-specifier-parser'

const GH_ALIASES: ReadonlySet<string> = new Set(['gh'])

describe('parseNamedRegistrySpecifier', () => {
  test('returns null on non-named-registry specifiers', () => {
    expect(parseNamedRegistrySpecifier('^1.0.0', GH_ALIASES)).toBeNull()
    expect(parseNamedRegistrySpecifier('1.0.0', GH_ALIASES)).toBeNull()
    expect(parseNamedRegistrySpecifier('latest', GH_ALIASES)).toBeNull()
    expect(parseNamedRegistrySpecifier('npm:foo', GH_ALIASES)).toBeNull()
    expect(parseNamedRegistrySpecifier('npm:@foo/bar', GH_ALIASES)).toBeNull()
    expect(parseNamedRegistrySpecifier('jsr:@foo/bar', GH_ALIASES)).toBeNull()
    expect(parseNamedRegistrySpecifier('catalog:', GH_ALIASES)).toBeNull()
    expect(parseNamedRegistrySpecifier('workspace:*', GH_ALIASES)).toBeNull()
  })

  test('does not intercept github: git shorthand (that scheme belongs to the git resolver)', () => {
    // `hosted-git-info` / `npm-package-arg` own the `github:` scheme as a GitHub git repository shortcut.
    // `github` is reserved at the resolver boundary, so it must not appear here — but even if a caller
    // accidentally passed it in, it wouldn't be in the built-in `gh` alias set.
    expect(parseNamedRegistrySpecifier('github:owner/repo', GH_ALIASES)).toBeNull()
    expect(parseNamedRegistrySpecifier('github:owner/repo#main', GH_ALIASES)).toBeNull()
    expect(parseNamedRegistrySpecifier('github:@acme/foo', GH_ALIASES)).toBeNull()
  })

  test('parses <alias>:<version_selector> when a scoped package alias is given', () => {
    expect(parseNamedRegistrySpecifier('gh:^1.0.0', GH_ALIASES, '@acme/foo')).toStrictEqual<NamedRegistrySpec>({
      pkgName: '@acme/foo',
      versionSelector: '^1.0.0',
      registryAlias: 'gh',
    })
    expect(parseNamedRegistrySpecifier('gh:1.0.0', GH_ALIASES, '@acme/foo')).toStrictEqual<NamedRegistrySpec>({
      pkgName: '@acme/foo',
      versionSelector: '1.0.0',
      registryAlias: 'gh',
    })
    expect(parseNamedRegistrySpecifier('gh:latest', GH_ALIASES, '@acme/foo')).toStrictEqual<NamedRegistrySpec>({
      pkgName: '@acme/foo',
      versionSelector: 'latest',
      registryAlias: 'gh',
    })
  })

  test('parses <alias>:@<owner>/<name>', () => {
    expect(parseNamedRegistrySpecifier('gh:@acme/foo', GH_ALIASES)).toStrictEqual<NamedRegistrySpec>({
      pkgName: '@acme/foo',
      registryAlias: 'gh',
    })
  })

  test('parses <alias>:@<owner>/<name>@<version_selector>', () => {
    expect(parseNamedRegistrySpecifier('gh:@acme/foo@^1.0.0', GH_ALIASES)).toStrictEqual<NamedRegistrySpec>({
      pkgName: '@acme/foo',
      versionSelector: '^1.0.0',
      registryAlias: 'gh',
    })
    expect(parseNamedRegistrySpecifier('gh:@acme/foo@1.0.0', GH_ALIASES)).toStrictEqual<NamedRegistrySpec>({
      pkgName: '@acme/foo',
      versionSelector: '1.0.0',
      registryAlias: 'gh',
    })
    expect(parseNamedRegistrySpecifier('gh:@acme/foo@latest', GH_ALIASES)).toStrictEqual<NamedRegistrySpec>({
      pkgName: '@acme/foo',
      versionSelector: 'latest',
      registryAlias: 'gh',
    })
  })

  test('preserves the original package name (no scope rewrite, unlike jsr)', () => {
    // Named registries publish the package under its original name, unlike the JSR
    // npm compatibility registry which remaps `@scope/name` to `@jsr/scope__name`.
    expect(parseNamedRegistrySpecifier('gh:@acme/foo@1.0.0', GH_ALIASES)?.pkgName).toBe('@acme/foo')
  })

  test('throws when the scope has no package name', () => {
    expect(() => parseNamedRegistrySpecifier('gh:@acme@^1.0.0', GH_ALIASES)).toThrow(expect.objectContaining({
      code: 'ERR_PNPM_INVALID_NAMED_REGISTRY_PACKAGE_NAME',
    }))
    expect(() => parseNamedRegistrySpecifier('gh:@acme', GH_ALIASES)).toThrow(expect.objectContaining({
      code: 'ERR_PNPM_INVALID_NAMED_REGISTRY_PACKAGE_NAME',
    }))
    expect(() => parseNamedRegistrySpecifier('gh:@acme/', GH_ALIASES)).toThrow(expect.objectContaining({
      code: 'ERR_PNPM_INVALID_NAMED_REGISTRY_PACKAGE_NAME',
    }))
  })

  test('does not claim <alias>:<version_selector> when no scoped alias is provided', () => {
    // No alias means we cannot know the package name — we must not hijack such specifiers.
    expect(parseNamedRegistrySpecifier('gh:^1.0.0', GH_ALIASES)).toBeNull()
  })

  test('does not claim <alias>:<version_selector> when the alias is not scoped', () => {
    // GitHub Packages names are always scoped. If the alias isn't, the bare specifier is
    // probably meant for another resolver.
    expect(parseNamedRegistrySpecifier('gh:^1.0.0', GH_ALIASES, 'foo')).toBeNull()
  })

  test('matches any alias in the configured set and reports it back to the caller', () => {
    expect(parseNamedRegistrySpecifier('work:@acme/foo@^1.0.0', new Set(['gh', 'work']))).toStrictEqual<NamedRegistrySpec>({
      pkgName: '@acme/foo',
      versionSelector: '^1.0.0',
      registryAlias: 'work',
    })
  })

  test('returns null when the alias is not in the configured set', () => {
    // Unrecognized prefixes must fall through so other resolvers (git, npm:, etc.) can try.
    expect(parseNamedRegistrySpecifier('work:@acme/foo', GH_ALIASES)).toBeNull()
  })

  test('includes the user-facing alias in error messages for user-defined aliases', () => {
    // When `work:@acme` fails validation, the user's alias must appear in the error
    // so they can find the offending specifier — not a generic `gh` reference.
    expect(() => parseNamedRegistrySpecifier('work:@acme', new Set(['work']))).toThrow(expect.objectContaining({
      code: 'ERR_PNPM_INVALID_NAMED_REGISTRY_PACKAGE_NAME',
      message: expect.stringContaining("'work:'"),
    }))
  })
})
