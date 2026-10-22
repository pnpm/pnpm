import fs from 'node:fs'
import { createGzip, gunzipSync, gzipSync } from 'node:zlib'

import { describe, expect, test } from '@jest/globals'
import { prepareEmpty } from '@pnpm/prepare'
import type { ExportedManifest } from '@pnpm/releasing.exportable-manifest'
import tar from 'tar-stream'

import {
  extractManifestFromPacked,
  extractPublishManifestFromPacked,
  isTarballPath,
  PublishArchiveMissingManifestError,
  type TarballPath,
} from '../../src/publish/extractManifestFromPacked.js'

async function createTarball (
  tarballPath: string,
  contents: Record<string, string | ExportedManifest>,
  nonFiles: Array<{ name: string, type: 'symlink' | 'directory', linkname?: string }> = []
): Promise<void> {
  const pack = tar.pack()

  for (const name in contents) {
    const content = contents[name]
    const textContent = typeof content === 'string' ? content : JSON.stringify(content, undefined, 2)
    pack.entry({ name }, textContent)
  }
  for (const { name, type, linkname } of nonFiles) {
    pack.entry({ name, type, linkname }, '')
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

describe('extractPublishManifestFromPacked', () => {
  test.each(['README.md', 'README', 'readme.markdown'])(
    'fills the manifest readme from tarball %s when the manifest lacks one',
    async (readmeFileName) => {
      prepareEmpty()

      const tarballPath: TarballPath = 'my-package.tgz'

      await createTarball(tarballPath, {
        'package/package.json': { name: 'hello-world', version: '0.0.0' },
        [`package/${readmeFileName}`]: '# Hello',
      })

      expect(await extractPublishManifestFromPacked(tarballPath)).toStrictEqual({
        name: 'hello-world',
        version: '0.0.0',
        readme: '# Hello',
      })
    }
  )

  test('prefers a Markdown README over bare README in a tarball', async () => {
    prepareEmpty()

    const tarballPath: TarballPath = 'my-package.tgz'

    await createTarball(tarballPath, {
      'package/README': '# Bare',
      'package/package.json': { name: 'hello-world', version: '0.0.0' },
      'package/readme.markdown': '# Markdown',
    })

    expect((await extractPublishManifestFromPacked(tarballPath)).readme).toBe('# Markdown')
  })

  test('keeps a Markdown README when bare README follows in a tarball', async () => {
    prepareEmpty()
    const tarballPath: TarballPath = 'my-package.tgz'

    await createTarball(tarballPath, {
      'package/readme.markdown': '# Markdown',
      'package/package.json': { name: 'hello-world', version: '0.0.0' },
      'package/README': '# Bare',
    })

    expect((await extractPublishManifestFromPacked(tarballPath)).readme).toBe('# Markdown')
  })

  test('prefers README.md when the tarball contains multiple README candidates', async () => {
    prepareEmpty()

    const tarballPath: TarballPath = 'my-package.tgz'

    await createTarball(tarballPath, {
      'package/readme.markdown': '# Fallback',
      'package/package.json': { name: 'hello-world', version: '0.0.0' },
      'package/README.md': '# Preferred',
      'package/README': '# Bare',
    })

    expect((await extractPublishManifestFromPacked(tarballPath)).readme).toBe('# Preferred')
  })

  test('prefers README.md over bare README in a tarball', async () => {
    prepareEmpty()
    const tarballPath: TarballPath = 'my-package.tgz'
    await createTarball(tarballPath, {
      'package/README': '# Bare',
      'package/package.json': { name: 'hello-world', version: '0.0.0' },
      'package/README.md': '# Preferred',
    })
    expect((await extractPublishManifestFromPacked(tarballPath)).readme).toBe('# Preferred')
  })

  test('picks the lowest name among Markdown READMEs in a tarball', async () => {
    prepareEmpty()
    const tarballPath: TarballPath = 'my-package.tgz'
    await createTarball(tarballPath, {
      'package/README.mdown': '# Mdown',
      'package/package.json': { name: 'hello-world', version: '0.0.0' },
      'package/README.markdown': '# Markdown',
    })
    expect((await extractPublishManifestFromPacked(tarballPath)).readme).toBe('# Markdown')
  })

  test('picks a lower-named README.MD that follows the manifest and README.md', async () => {
    prepareEmpty()
    const tarballPath: TarballPath = 'my-package.tgz'
    await createTarball(tarballPath, {
      'package/package.json': { name: 'hello-world', version: '0.0.0' },
      'package/README.md': '# First',
      'package/README.MD': '# Upper',
    })
    expect((await extractPublishManifestFromPacked(tarballPath)).readme).toBe('# Upper')
  })

  test('ignores non-file README entries in a tarball', async () => {
    prepareEmpty()
    const tarballPath: TarballPath = 'my-package.tgz'
    await createTarball(tarballPath, {
      'package/package.json': { name: 'hello-world', version: '0.0.0' },
      'package/README': '# Bare',
    }, [
      { name: 'package/README.md', type: 'symlink', linkname: 'README' },
      { name: 'package/readme.markdown', type: 'directory' },
    ])
    expect((await extractPublishManifestFromPacked(tarballPath)).readme).toBe('# Bare')
  })

  test.each(['README.md', 'README'])('keeps a readme already declared in the manifest with %s', async (readmeName) => {
    prepareEmpty()

    const tarballPath: TarballPath = 'my-package.tgz'

    await createTarball(tarballPath, {
      'package/package.json': { name: 'hello-world', version: '0.0.0', readme: 'embedded' },
      [`package/${readmeName}`]: '# Hello',
    })

    expect((await extractPublishManifestFromPacked(tarballPath)).readme).toBe('embedded')
  })

  test('leaves readme unset when the tarball has no README', async () => {
    prepareEmpty()

    const tarballPath: TarballPath = 'my-package.tgz'

    await createTarball(tarballPath, {
      'package/package.json': { name: 'hello-world', version: '0.0.0' },
    })

    expect((await extractPublishManifestFromPacked(tarballPath)).readme).toBeUndefined()
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

test.each(['package/package.json', 'package/README.md', 'package/README.\x1b[31m\x9b31m.md'])('rejects oversized buffered entry %s before reading its payload', async (filename) => {
  prepareEmpty()
  const tarballPath: TarballPath = 'oversized.tgz'
  await createTarball(tarballPath, { 'package/placeholder': '' })
  const header = gunzipSync(fs.readFileSync(tarballPath)).subarray(0, 512)
  header.fill(0, 0, 100)
  header.write(filename, 0, 'utf8')
  header.write((64 * 1024 * 1024 + 1).toString(8).padStart(11, '0') + '\0', 124, 'ascii')
  header.fill(0x20, 148, 156)
  const checksum = header.reduce((total, byte) => total + byte, 0)
  header.write(checksum.toString(8).padStart(6, '0') + '\0 ', 148, 'ascii')
  fs.writeFileSync(tarballPath, gzipSync(header))
  const extraction = extractPublishManifestFromPacked(tarballPath)
  await expect(extraction).rejects.toMatchObject({
    code: 'ERR_PNPM_PUBLISH_EXTRACT_MANIFEST_READ',
    message: expect.stringContaining(filename.replaceAll('\x1b', '\\x1B').replaceAll('\x9b', '\\x9B')),
  })
  await expect(extraction).rejects.toHaveProperty('message', expect.not.stringContaining('\x1b'))
  await expect(extraction).rejects.toHaveProperty('message', expect.not.stringContaining('\x9b'))
  if (filename.endsWith('package.json')) {
    await expect(extractManifestFromPacked(tarballPath)).rejects.toMatchObject({
      code: 'ERR_PNPM_PUBLISH_EXTRACT_MANIFEST_READ',
    })
  }
})
