import npmPacklist from 'npm-packlist'

export async function packlist (pkgDir: string): Promise<string[]> {
  const files = await npmPacklist({ path: pkgDir })
  // There's a bug in the npm-packlist version that we use,
  // it sometimes returns duplicates.
  // Related issue: https://github.com/pnpm/pnpm/issues/6997
  // Unfortunately, we cannot upgrade the library
  // newer versions of npm-packlist are very slow.
  return Array.from(new Set(files.map((file) => file.replace(/^\.[/\\]/, ''))))
}
