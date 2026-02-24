import fs from 'fs'
import { createGzip } from 'zlib'
import tar from 'tar-stream'
import { type ExportedManifest } from '@pnpm/exportable-manifest'
import { prepareEmpty } from '@pnpm/prepare'
import {
  type TarballPath,
  PublishArchiveMissingManifestError,
  isTarballPath,
  extractManifestFromPacked,
} from '../src/extractManifestFromPacked.js'

async function createTarball (tarballPath: string, contents: Record<string, string | ExportedManifest>): Promise<void> {
  const pack = tar.pack()

  for (const name in contents) {
    const content = contents[name]
    const textContent = typeof content === 'string' ? content : JSON.stringify(content, undefined, 2)
    pack.entry({ name }, textContent)
  }

  const tarball = fs.createWriteStream(tarballPath)
  pack.pipe(createGzip()).pipe(tarball)
  pack.finalize()

  return new Promise((resolve, reject) => {
    tarball.on('close', resolve)
    tarball.on('error', reject)
  })
}

describe('extractManifestFromPacked', () => {
  test('extracts manifest from a packed package', async () => {
    prepareEmpty()

    const tarballPath: TarballPath = 'my-package.tgz'

    const manifest: ExportedManifest = {
      name: 'hello-world',
      version: '0.0.0',
    }

    await createTarball(tarballPath, {
      'package/lib/foo.js': 'hello',
      'package/lib/bar.js': 'world',
      'package/package.json': manifest,
      'package/README.md': 'example',
    })

    expect(await extractManifestFromPacked(tarballPath)).toStrictEqual(manifest)
  })

  test('errors when manifest does not exist', async () => {
    prepareEmpty()

    const tarballPath: TarballPath = 'my-package.tgz'

    await createTarball(tarballPath, {
      'package/lib/foo.js': 'hello',
      'package/lib/bar.js': 'world',
      'package/README.md': 'example',
    })

    const promise = extractManifestFromPacked(tarballPath)
    await expect(promise).rejects.toBeInstanceOf(PublishArchiveMissingManifestError)
    await expect(promise).rejects.toStrictEqual(new PublishArchiveMissingManifestError(tarballPath))
    await expect(promise).rejects.toMatchObject({
      code: 'ERR_PNPM_PUBLISH_ARCHIVE_MISSING_MANIFEST',
      tarballPath,
    })
  })

  test('errors when the manifest is not placed in the correct location', async () => {
    prepareEmpty()

    const tarballPath: TarballPath = 'my-package.tgz'

    const manifest: ExportedManifest = {
      name: 'hello-world',
      version: '0.0.0',
    }

    await createTarball(tarballPath, {
      'lib/foo.js': 'hello',
      'lib/bar.js': 'world',
      'package.json': manifest,
      'README.md': 'example',
    })

    const promise = extractManifestFromPacked(tarballPath)
    await expect(promise).rejects.toBeInstanceOf(PublishArchiveMissingManifestError)
    await expect(promise).rejects.toStrictEqual(new PublishArchiveMissingManifestError(tarballPath))
    await expect(promise).rejects.toMatchObject({
      code: 'ERR_PNPM_PUBLISH_ARCHIVE_MISSING_MANIFEST',
      tarballPath,
    })
  })
})

describe('isTarballPath', () => {
  test('returns true for .tgz', () => {
    expect(isTarballPath('foo/bar.tgz')).toBe(true)
    expect(isTarballPath('foo.tgz')).toBe(true)
  })

  test('returns true for .tar.gz', () => {
    expect(isTarballPath('foo/bar.tar.gz')).toBe(true)
    expect(isTarballPath('foo.tar.gz')).toBe(true)
  })

  test('returns false for non tarball extensions', () => {
    expect(isTarballPath('foo/bar')).toBe(false)
    expect(isTarballPath('foo/bar.tar')).toBe(false)
    expect(isTarballPath('foo/bar.gz')).toBe(false)
    expect(isTarballPath('tgz')).toBe(false)
    expect(isTarballPath('tar.gz')).toBe(false)
  })
})
