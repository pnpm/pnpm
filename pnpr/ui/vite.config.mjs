import { createRequire } from 'node:module'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const require = createRequire(import.meta.url)
const appDir = path.dirname(require.resolve('@pnpm/pnpr.apps.pnpr-web/package.json'))

// Mirrors the app component's own vite.config.mjs, which explains the `tty`
// alias and the `process` stand-ins.
export default {
  // pnpr inserts a `<base>` that points at the UI root, so asset URLs stay
  // relative and work behind any path prefix.
  base: './',
  build: {
    outDir: fileURLToPath(new URL('../npm/pnpr-ui/dist', import.meta.url)),
    emptyOutDir: true,
  },
  resolve: {
    alias: {
      tty: path.join(appDir, 'tty-stub.mjs'),
    },
  },
  define: {
    'process.env.PNPR_BASE_URL': 'undefined',
    'process.stderr': JSON.stringify({ fd: 2, isTTY: false }),
    'process.stdout': JSON.stringify({ fd: 1, isTTY: false }),
  },
}
