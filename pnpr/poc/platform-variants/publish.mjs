// Publish a primary archive and its platform variants to pnpr in one request.
//
// usage: node publish.mjs <registry> <token> <variants.json> [--tag latest]
import fs from 'node:fs'
import crypto from 'node:crypto'
import { execFileSync } from 'node:child_process'

const [registry, token, descriptor] = process.argv.slice(2)
const spec = JSON.parse(fs.readFileSync(descriptor, 'utf8'))

function readPackageJson (tgz) {
  return JSON.parse(execFileSync('tar', ['xzOf', tgz, 'package/package.json'], { encoding: 'utf8' }))
}

const primaryBytes = fs.readFileSync(spec.primary.file)
const manifest = readPackageJson(spec.primary.file)
const base = spec.name.split('/').pop()
const tgzName = `${base}-${spec.version}.tgz`
manifest._id = `${spec.name}@${spec.version}`
manifest.dist = {
  integrity: spec.primary.integrity,
  shasum: crypto.createHash('sha1').update(primaryBytes).digest('hex'),
  tarball: `${registry}${spec.name}/-/${tgzName}`,
}
if (spec.variants.length > 0) {
  manifest.pnprVariants = spec.variants
    .filter(v => !v.undeclared)
    .map(v => ({ tag: v.tag, platforms: v.platforms, integrity: v.integrity }))
}
const attachments = {
  [tgzName]: { content_type: 'application/octet-stream', data: primaryBytes.toString('base64'), length: primaryBytes.length },
}
for (const v of spec.variants.filter(v => !v.missing)) {
  const bytes = fs.readFileSync(v.file)
  attachments[`${base}-${spec.version}~${v.tag}.tgz`] = {
    content_type: 'application/octet-stream', data: bytes.toString('base64'), length: bytes.length,
  }
}
const body = {
  _id: spec.name,
  name: spec.name,
  'dist-tags': { [spec.distTag ?? 'latest']: spec.version },
  versions: { [spec.version]: manifest },
  _attachments: attachments,
}
const res = await fetch(registry + spec.name.replaceAll('/', '%2f'), {
  method: 'PUT',
  headers: { 'content-type': 'application/json', authorization: `Bearer ${token}` },
  body: JSON.stringify(body),
})
const text = await res.text()
console.log(`${spec.name}@${spec.version}: ${res.status} ${text.slice(0, 400)}`)
if (!res.ok) process.exit(1)
