import path from 'node:path'
import { loadJsonFileSync } from 'load-json-file'

test('published manifest should not contain \'peerDependencies\' or \'optionalDependencies\'', () => {
  const manifest: Record<string, unknown> = loadJsonFileSync(path.join(import.meta.dirname, '../package.json'))

  // A similar check is ran as a part of the beforePacking hook, but adding as a
  // Jest test as well to make sure this error is caught before attempting to
  // release a new version of pnpm.
  for (const depKind of ['peerDependencies', 'optionalDependencies']) {
    if (Object.keys(manifest[depKind] ?? {}).length > 0) {
      throw new Error(`The main 'pnpm' package should not declare '${depKind}'. Consider moving to 'devDependencies' if the dependency can be included in the esbuild bundle, or to 'dependencies' if the dependency needs to be externalized and resolved at runtime.`)
    }
  }
})
