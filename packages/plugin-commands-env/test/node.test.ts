import AdmZip from 'adm-zip'
import { Response } from 'node-fetch'
import path from 'path'
import { Readable } from 'stream'
import { node } from '@pnpm/plugin-commands-env'
import { tempDir } from '@pnpm/prepare'

test('check API (placeholder test)', async () => {
  expect(typeof node.getNodeDir).toBe('function')
})

test('install Node uses node-mirror:release option', async () => {
  tempDir()
  const configDir = path.resolve('config')

  const nodeMirrorRelease = 'https://pnpm-node-mirror-test.localhost/download/release'
  const opts: node.NvmNodeCommandOptions = {
    bin: process.cwd(),
    configDir,
    global: true,
    pnpmHomeDir: process.cwd(),
    rawConfig: {
      'node-mirror:release': nodeMirrorRelease,
    },
    useNodeVersion: '16.4.0',
  }

  const fetchMock = jest.fn(async (url: string) => {
    if (url.endsWith('.zip')) {
      // The Windows code path for pnpm's node bootstrapping expects a subdir
      // within the .zip file.
      const pkgName = path.basename(url, '.zip')
      const zip = new AdmZip()
      zip.addFile(`${pkgName}/dummy-file`, Buffer.from('test'))

      return new Response(Readable.from(zip.toBuffer()))
    }

    return new Response(Readable.from(Buffer.alloc(0)))
  })

  await node.getNodeDir(fetchMock, opts)

  for (const call of fetchMock.mock.calls) {
    expect(call[0]).toMatch(nodeMirrorRelease)
  }
})
