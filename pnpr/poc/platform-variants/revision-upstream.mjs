// A registry that serves revpkg@1.0.0 with an original (r0) and a patched
// replacement (r1, selected), in the revision protocol's npm form.
import http from 'node:http'; import fs from 'node:fs'; import crypto from 'node:crypto'
const dir = new URL('./build/', import.meta.url).pathname
const tars = { 0: fs.readFileSync(dir + 'orig.tar'), 1: fs.readFileSync(dir + 'patched.tar') }
const integrity = b => 'sha512-' + crypto.createHash('sha512').update(b).digest('base64')
const port = Number(process.argv[2]), base = `http://127.0.0.1:${port}`
const record = r => ({ revision: r, integrity: integrity(tars[r]), tarball: `${base}/revpkg@1.0.0+r${r}`, manifest: { main: 'index.js' } })
const doc = { name: 'revpkg', 'dist-tags': { latest: '1.0.0' }, time: { '1.0.0': '2026-01-01T00:00:00.000Z' },
  versions: { '1.0.0': { name: 'revpkg', version: '1.0.0', main: 'index.js', dist: { ...record(1), revisions: [record(0), record(1)] } } } }
delete doc.versions['1.0.0'].dist.manifest
http.createServer((req, res) => {
  const m = req.url.match(/^\/revpkg@1\.0\.0\+r(\d)$/)
  if (req.url === '/revpkg') { res.setHeader('content-type', 'application/json'); return res.end(JSON.stringify(doc)) }
  if (m && tars[m[1]]) { res.setHeader('content-type', 'application/x-tar'); return res.end(tars[m[1]]) }
  res.statusCode = 404; res.end()
}).listen(port)
