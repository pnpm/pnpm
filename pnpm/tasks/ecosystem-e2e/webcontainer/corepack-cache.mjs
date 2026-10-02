import { execFile } from 'node:child_process'
import { createHash } from 'node:crypto'
import fs from 'node:fs'
import http from 'node:http'
import os from 'node:os'
import path from 'node:path'
import { promisify } from 'node:util'

import { unpackWrapper } from './wrapper-fixture.mjs'

const [archiveArgument, destinationArgument] = process.argv.slice(2)
if (!archiveArgument || !destinationArgument) throw new Error('Usage: corepack-cache.mjs <wrapper.tgz> <cache.tgz>')
const archive = fs.readFileSync(archiveArgument)
const unpacked = unpackWrapper(path.resolve(archiveArgument))
const manifest = JSON.parse(fs.readFileSync(unpacked.files.get('corepack/package/package.json'), 'utf8'))
const digest = createHash('sha512').update(archive).digest()
const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-corepack-cache-'))
const server = http.createServer(servePackage)
try {
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve))
  const reference = `pnpm@${manifest.version}+sha512.${digest.toString('hex')}`
  const result = await promisify(execFile)('pnpm', [
    'dlx', 'corepack@0.36.0', 'pack', reference, '--output', path.resolve(destinationArgument),
  ], { cwd: directory, env: {
    ...process.env,
    COREPACK_HOME: path.join(directory, 'cache'),
    COREPACK_NPM_REGISTRY: `http://127.0.0.1:${server.address().port}`,
    COREPACK_DEFAULT_TO_LATEST: '0',
  } })
  process.stdout.write(result.stdout)
} finally {
  server.close()
  unpacked.close()
  fs.rmSync(directory, { recursive: true, force: true })
}

function servePackage (request, response) {
  if (request.url === '/pnpm.tgz') {
    response.end(archive)
  } else if (request.url === `/pnpm/${manifest.version}`) {
    response.setHeader('content-type', 'application/json')
    response.end(JSON.stringify({ ...manifest, dist: {
      tarball: `http://127.0.0.1:${server.address().port}/pnpm.tgz`,
      integrity: `sha512-${digest.toString('base64')}`,
    } }))
  } else {
    response.statusCode = 404
    response.end('{}')
  }
}
