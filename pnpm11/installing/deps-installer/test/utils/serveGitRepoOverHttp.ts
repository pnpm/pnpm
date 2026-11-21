import fs from 'node:fs'
import http from 'node:http'
import type { AddressInfo } from 'node:net'
import path from 'node:path'

import { safeExeca as execa } from 'execa'

/**
 * Serves a bare clone of the committed repository at `repoDir` over the dumb
 * HTTP protocol on loopback.
 *
 * @returns a `git+http:` URL pinned to `commit`, and a function that stops the server.
 */
export async function serveGitRepoOverHttp (
  repoDir: string,
  commit: string
): Promise<{ gitSpec: string, close: () => Promise<void> }> {
  const servedRoot = fs.mkdtempSync(path.join(path.dirname(repoDir), 'served-'))
  const bareName = `${path.basename(repoDir)}.git`
  const bareDir = path.join(servedRoot, bareName)
  await execa('git', ['clone', '-q', '--bare', repoDir, bareDir])
  await execa('git', ['update-server-info'], { cwd: bareDir })
  const server = http.createServer((req, res) => {
    const filePath = path.join(servedRoot, decodeURIComponent((req.url ?? '/').split('?')[0]))
    if (!filePath.startsWith(bareDir + path.sep) || !fs.existsSync(filePath) || fs.statSync(filePath).isDirectory()) {
      res.writeHead(404).end()
      return
    }
    res.writeHead(200, { 'content-type': 'application/octet-stream' })
    fs.createReadStream(filePath).pipe(res)
  })
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
  const { port } = server.address() as AddressInfo
  return {
    gitSpec: `git+http://127.0.0.1:${port}/${bareName}#${commit}`,
    close: async () => new Promise<void>((resolve) => server.close(() => resolve())),
  }
}
