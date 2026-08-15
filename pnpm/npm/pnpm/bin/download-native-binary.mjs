// Fetch the host's native pnpm binary straight from the npm registry, for
// `bin/pnpm.mjs`. Only Corepack needs this: every package manager installs the
// `@pnpm/exe.<target>` optional dependency instead, and Corepack installs no
// dependencies at all.
//
// Zero runtime dependencies (Corepack would not install them either), and the
// same registry environment variables Corepack itself honours, so a run that
// could reach the registry for the `pnpm` tarball can reach it for the binary.
import { Buffer } from 'node:buffer'
import { createHash, randomBytes } from 'node:crypto'
import fs from 'node:fs'
import process from 'node:process'
import zlib from 'node:zlib'

const DEFAULT_REGISTRY = 'https://registry.npmjs.org'
const TARBALL_ROOT = 'package/'
// Strongest first: the published integrity may list several, and a weak one
// must not be able to stand in for a strong one.
const SUPPORTED_HASHES = ['sha512', 'sha384', 'sha256', 'sha1']
const TAR_BLOCK_SIZE = 512

/**
 * Download `packageName@version`, extract `binFile` from it and place it at
 * `destPath` (atomically, so a concurrent run either sees the old state or the
 * complete binary).
 *
 * @param {object} opts
 * @param {string} opts.packageName Platform package, e.g. `@pnpm/exe.linux-x64`.
 * @param {string} opts.version Exact version, taken from the wrapper's `optionalDependencies`.
 * @param {string} opts.binFile Path of the binary inside the package.
 * @param {string} opts.destPath Absolute path to create.
 */
export async function downloadNativeBinary ({ packageName, version, binFile, destPath }) {
  const registry = registryUrl()
  const metadata = await fetchJson(`${registry}/${packageName.replaceAll('/', '%2F')}/${version}`)
  const dist = metadata?.dist
  if (typeof dist?.tarball !== 'string') {
    throw new Error(`The registry returned no tarball URL for ${packageName}@${version}`)
  }

  const tarball = Buffer.from(await fetchBuffer(rehostTarballUrl(dist.tarball, registry)))
  verifyIntegrity(tarball, dist, `${packageName}@${version}`)

  const binary = readTarEntry(zlib.gunzipSync(tarball), `${TARBALL_ROOT}${binFile}`)
  if (binary == null) {
    throw new Error(`The ${packageName}@${version} tarball contains no ${binFile}`)
  }

  // Unpredictable name + exclusive create: the destination directory is shared
  // (a Corepack cache, a `node_modules`), so the temp file must not be a path
  // something else can plant a symlink at and have us write through it.
  const tempPath = `${destPath}.${randomBytes(6).toString('hex')}.tmp`
  let created = false
  try {
    const file = fs.openSync(tempPath, 'wx', 0o755)
    created = true
    try {
      fs.writeFileSync(file, binary)
    } finally {
      fs.closeSync(file)
    }
    fs.renameSync(tempPath, destPath)
  } catch (err) {
    if (created) {
      fs.rmSync(tempPath, { force: true })
    }
    throw new Error(`Could not write the pnpm binary to ${destPath}: ${err.message}`)
  }
}

function registryUrl () {
  // Trailing slashes stripped so a `COREPACK_NPM_REGISTRY` with one does not
  // produce a double slash (some registries answer `//package` with a 404).
  return (process.env.COREPACK_NPM_REGISTRY || DEFAULT_REGISTRY).replace(/\/+$/, '')
}

// Registries that proxy npm hand back npmjs.org tarball URLs; Corepack rewrites
// those to the configured registry, and so must we, or the download escapes an
// air-gapped mirror that the metadata request stayed inside of. Matched by
// origin rather than by prefix, so a host that merely starts with npm's — say
// `registry.npmjs.org.example.com` — is left alone instead of being spliced
// onto the configured registry.
function rehostTarballUrl (tarballUrl, registry) {
  const url = new URL(tarballUrl)
  if (url.origin !== new URL(DEFAULT_REGISTRY).origin) {
    return tarballUrl
  }
  return `${registry}${url.pathname}${url.search}`
}

async function fetchJson (url) {
  const response = await request(url, {
    // Abbreviated metadata carries `dist`, and is much smaller than the full doc.
    accept: 'application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8',
  })
  return response.json()
}

async function fetchBuffer (url) {
  const response = await request(url)
  return response.arrayBuffer()
}

async function request (url, { accept } = {}) {
  if (process.env.COREPACK_ENABLE_NETWORK === '0') {
    throw new Error(`Network access is disabled by the environment; cannot reach ${url}`)
  }

  const headers = accept == null ? {} : { accept }
  const authorization = authorizationFor(url)
  if (authorization != null) {
    headers.authorization = authorization
  }

  let response
  try {
    response = await fetch(url, { headers })
  } catch (err) {
    throw new Error(`Request to ${url} failed: ${err.message}`, { cause: err })
  }
  if (!response.ok) {
    throw new Error(`Request to ${url} failed with HTTP ${response.status}`)
  }
  return response
}

// Credentials only travel to the registry they were configured for, never to a
// tarball host that redirected off it.
function authorizationFor (url) {
  const { COREPACK_NPM_TOKEN, COREPACK_NPM_USERNAME, COREPACK_NPM_PASSWORD } = process.env
  if (new URL(url).origin !== new URL(registryUrl()).origin) {
    return null
  }
  if (COREPACK_NPM_TOKEN) {
    return `Bearer ${COREPACK_NPM_TOKEN}`
  }
  if (COREPACK_NPM_USERNAME && COREPACK_NPM_PASSWORD) {
    const credentials = Buffer.from(`${COREPACK_NPM_USERNAME}:${COREPACK_NPM_PASSWORD}`, 'utf8')
    return `Basic ${credentials.toString('base64')}`
  }
  return null
}

function verifyIntegrity (tarball, dist, label) {
  if (typeof dist.integrity === 'string') {
    // Subresource Integrity: whitespace-separated `<algorithm>-<base64>` entries.
    const digests = new Map()
    for (const entry of dist.integrity.split(/\s+/).filter(Boolean)) {
      const separator = entry.indexOf('-')
      digests.set(entry.slice(0, separator), entry.slice(separator + 1))
    }
    for (const algorithm of SUPPORTED_HASHES) {
      const expected = digests.get(algorithm)
      if (expected == null) continue
      const actual = createHash(algorithm).update(tarball).digest('base64')
      if (actual !== expected) {
        throw new Error(`Integrity check failed for ${label}: expected ${algorithm}-${expected}, got ${algorithm}-${actual}`)
      }
      return
    }
  }
  if (typeof dist.shasum === 'string') {
    const actual = createHash('sha1').update(tarball).digest('hex')
    if (actual !== dist.shasum) {
      throw new Error(`Integrity check failed for ${label}: expected sha1 ${dist.shasum}, got ${actual}`)
    }
    return
  }
  throw new Error(`The registry published no checksum for ${label}, so it cannot be verified`)
}

/**
 * Read one file out of an uncompressed tar archive.
 *
 * A hand-rolled reader keeps the wrapper dependency-free. npm tarballs are
 * plain ustar with short, ASCII paths, so only regular files matter: any other
 * entry type (a `pax` header, a directory) is skipped by its recorded size.
 *
 * @param {Buffer} tar
 * @param {string} wantedPath
 * @returns {Buffer | null}
 */
function readTarEntry (tar, wantedPath) {
  let offset = 0
  while (offset + TAR_BLOCK_SIZE <= tar.length) {
    const header = tar.subarray(offset, offset + TAR_BLOCK_SIZE)
    // Two zero-filled blocks end the archive; one is enough to stop reading.
    if (header[0] === 0) break

    const name = readTarString(header, 0, 100)
    const prefix = readTarString(header, 345, 155)
    const size = parseInt(readTarString(header, 124, 12).trim() || '0', 8)
    const typeFlag = String.fromCharCode(header[156])
    const isFile = typeFlag === '0' || typeFlag === '\0'
    const entryPath = prefix === '' ? name : `${prefix}/${name}`

    offset += TAR_BLOCK_SIZE
    if (isFile && entryPath === wantedPath) {
      if (offset + size > tar.length) {
        throw new Error(`The tarball ends before ${wantedPath} does; it is truncated`)
      }
      return tar.subarray(offset, offset + size)
    }
    offset += Math.ceil(size / TAR_BLOCK_SIZE) * TAR_BLOCK_SIZE
  }
  return null
}

function readTarString (header, start, length) {
  const field = header.subarray(start, start + length)
  const end = field.indexOf(0)
  return field.toString('utf8', 0, end === -1 ? field.length : end)
}
