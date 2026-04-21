import { describe, expect, it } from '@jest/globals'
import type { LockfileObject } from '@pnpm/lockfile.types'
import type { PackageFilesIndex } from '@pnpm/store.cafs'
import { packForStorage } from '@pnpm/store.index'
import { computeDiff, type IntegrityEntry } from 'pnpm-agent'

function createIntegrityIndex (entries: Record<string, PackageFilesIndex>): Map<string, IntegrityEntry> {
  return new Map(Object.entries(entries).map(([k, v]) => [k, {
    decoded: v,
    rawBuffer: packForStorage(v) as Uint8Array,
  }]))
}

describe('computeDiff', () => {
  const storeDir = '/tmp/test-store'

  it('identifies all files as missing when client store is empty', () => {
    const lockfile: LockfileObject = {
      lockfileVersion: '9.0',
      importers: {},
      packages: {
        '/is-positive/1.0.0': {
          resolution: { integrity: 'sha512-pkg1' },
        },
      },
    } as any // eslint-disable-line @typescript-eslint/no-explicit-any

    const integrityIndex = createIntegrityIndex({
      'sha512-pkg1': {
        algo: 'sha512',
        files: new Map([
          ['index.js', { digest: 'aaa111', size: 100, mode: 0o644, checkedAt: 0 }],
          ['package.json', { digest: 'bbb222', size: 50, mode: 0o644, checkedAt: 0 }],
        ]),
      },
    })

    const { metadata, missingFiles } = computeDiff(
      lockfile,
      [], // empty store
      integrityIndex,
      storeDir
    )

    expect(metadata.stats.totalPackages).toBe(1)
    expect(metadata.stats.alreadyInStore).toBe(0)
    expect(metadata.stats.packagesToFetch).toBe(1)
    expect(metadata.stats.filesToDownload).toBe(2)
    expect(metadata.stats.filesAlreadyInCafs).toBe(0)
    expect(missingFiles).toHaveLength(2)
    expect(missingFiles.map(f => f.digest).sort()).toEqual(['aaa111', 'bbb222'])
  })

  it('skips packages the client already has', () => {
    const lockfile: LockfileObject = {
      lockfileVersion: '9.0',
      importers: {},
      packages: {
        '/is-positive/1.0.0': {
          resolution: { integrity: 'sha512-pkg1' },
        },
        '/is-negative/1.0.0': {
          resolution: { integrity: 'sha512-pkg2' },
        },
      },
    } as any // eslint-disable-line @typescript-eslint/no-explicit-any

    const integrityIndex = createIntegrityIndex({
      'sha512-pkg1': {
        algo: 'sha512',
        files: new Map([
          ['index.js', { digest: 'aaa111', size: 100, mode: 0o644, checkedAt: 0 }],
        ]),
      },
      'sha512-pkg2': {
        algo: 'sha512',
        files: new Map([
          ['index.js', { digest: 'ccc333', size: 200, mode: 0o644, checkedAt: 0 }],
        ]),
      },
    })

    const { metadata, missingFiles } = computeDiff(
      lockfile,
      ['sha512-pkg1'], // client has pkg1
      integrityIndex,
      storeDir
    )

    expect(metadata.stats.totalPackages).toBe(2)
    expect(metadata.stats.alreadyInStore).toBe(1)
    expect(metadata.stats.packagesToFetch).toBe(1)
    expect(metadata.stats.filesToDownload).toBe(1)
    expect(missingFiles).toHaveLength(1)
    expect(missingFiles[0].digest).toBe('ccc333')
  })

  it('deduplicates files shared across packages', () => {
    const sharedDigest = 'shared_license_hash'

    const lockfile: LockfileObject = {
      lockfileVersion: '9.0',
      importers: {},
      packages: {
        '/pkg-a/1.0.0': {
          resolution: { integrity: 'sha512-pkgA' },
        },
        '/pkg-b/1.0.0': {
          resolution: { integrity: 'sha512-pkgB' },
        },
      },
    } as any // eslint-disable-line @typescript-eslint/no-explicit-any

    const integrityIndex = createIntegrityIndex({
      'sha512-pkgA': {
        algo: 'sha512',
        files: new Map([
          ['index.js', { digest: 'unique_a', size: 100, mode: 0o644, checkedAt: 0 }],
          ['LICENSE', { digest: sharedDigest, size: 1089, mode: 0o644, checkedAt: 0 }],
        ]),
      },
      'sha512-pkgB': {
        algo: 'sha512',
        files: new Map([
          ['index.js', { digest: 'unique_b', size: 200, mode: 0o644, checkedAt: 0 }],
          ['LICENSE', { digest: sharedDigest, size: 1089, mode: 0o644, checkedAt: 0 }],
        ]),
      },
    })

    const { metadata, missingFiles } = computeDiff(
      lockfile,
      [],
      integrityIndex,
      storeDir
    )

    // 4 total file references, but sharedDigest only sent once
    expect(metadata.stats.filesInNewPackages).toBe(4)
    expect(metadata.stats.filesToDownload).toBe(3) // unique_a + unique_b + shared (once)
    expect(metadata.stats.filesAlreadyInCafs).toBe(1) // shared deduped
    expect(missingFiles).toHaveLength(3)

    const digestsSent = missingFiles.map(f => f.digest)
    expect(digestsSent).toContain(sharedDigest)
    expect(digestsSent.filter(d => d === sharedDigest)).toHaveLength(1)
  })

  it('detects file-level dedup across store packages and new packages', () => {
    // Client has pkg-old which shares a file digest with pkg-new
    const lockfile: LockfileObject = {
      lockfileVersion: '9.0',
      importers: {},
      packages: {
        '/pkg-new/2.0.0': {
          resolution: { integrity: 'sha512-new' },
        },
      },
    } as any // eslint-disable-line @typescript-eslint/no-explicit-any

    const integrityIndex = createIntegrityIndex({
      'sha512-old': {
        algo: 'sha512',
        files: new Map([
          ['index.js', { digest: 'old_unique', size: 100, mode: 0o644, checkedAt: 0 }],
          ['utils.js', { digest: 'shared_util', size: 500, mode: 0o644, checkedAt: 0 }],
        ]),
      },
      'sha512-new': {
        algo: 'sha512',
        files: new Map([
          ['index.js', { digest: 'new_unique', size: 150, mode: 0o644, checkedAt: 0 }],
          ['utils.js', { digest: 'shared_util', size: 500, mode: 0o644, checkedAt: 0 }],
        ]),
      },
    })

    const { metadata, missingFiles } = computeDiff(
      lockfile,
      ['sha512-old'], // client has pkg-old
      integrityIndex,
      storeDir
    )

    // pkg-new needs 2 files, but shared_util already in store via pkg-old
    expect(metadata.stats.packagesToFetch).toBe(1)
    expect(metadata.stats.filesInNewPackages).toBe(2)
    expect(metadata.stats.filesToDownload).toBe(1) // only new_unique
    expect(metadata.stats.filesAlreadyInCafs).toBe(1) // shared_util
    expect(missingFiles).toHaveLength(1)
    expect(missingFiles[0].digest).toBe('new_unique')
  })

  it('handles executable files correctly', () => {
    const lockfile: LockfileObject = {
      lockfileVersion: '9.0',
      importers: {},
      packages: {
        '/has-bin/1.0.0': {
          resolution: { integrity: 'sha512-bin' },
        },
      },
    } as any // eslint-disable-line @typescript-eslint/no-explicit-any

    const integrityIndex = createIntegrityIndex({
      'sha512-bin': {
        algo: 'sha512',
        files: new Map([
          ['bin/cli.js', { digest: 'exec_hash', size: 300, mode: 0o755, checkedAt: 0 }],
          ['lib/index.js', { digest: 'lib_hash', size: 200, mode: 0o644, checkedAt: 0 }],
        ]),
      },
    })

    const { missingFiles } = computeDiff(
      lockfile,
      [],
      integrityIndex,
      storeDir
    )

    const execFile = missingFiles.find(f => f.digest === 'exec_hash')!
    const libFile = missingFiles.find(f => f.digest === 'lib_hash')!

    expect(execFile.executable).toBe(true)
    expect(libFile.executable).toBe(false)

    // CAFS paths should differ for exec vs non-exec
    expect(execFile.cafsPath).toContain('-exec')
    expect(libFile.cafsPath).not.toContain('-exec')
  })

  it('includes package file indexes in metadata', () => {
    const lockfile: LockfileObject = {
      lockfileVersion: '9.0',
      importers: {},
      packages: {
        '/my-pkg/1.0.0': {
          resolution: { integrity: 'sha512-test' },
        },
      },
    } as any // eslint-disable-line @typescript-eslint/no-explicit-any

    const integrityIndex = createIntegrityIndex({
      'sha512-test': {
        algo: 'sha512',
        files: new Map([
          ['index.js', { digest: 'hash1', size: 100, mode: 0o644, checkedAt: 0 }],
          ['README.md', { digest: 'hash2', size: 50, mode: 0o644, checkedAt: 0 }],
        ]),
      },
    })

    const { packageIndexBuffers } = computeDiff(lockfile, [], integrityIndex, storeDir)

    const entry = packageIndexBuffers.get('/my-pkg/1.0.0')
    expect(entry).toBeTruthy()
    expect(entry!.integrity).toBe('sha512-test')
    expect(entry!.rawBuffer).toBeInstanceOf(Uint8Array)
    expect(entry!.rawBuffer.length).toBeGreaterThan(0)
  })
})
