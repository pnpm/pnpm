# @pnpm/store.pkg-finder

Resolves a package's file index from the content-addressable store (CAFS) and returns a uniform `Map<string, string>` mapping filenames to absolute paths on disk.

## Usage

```ts
import { readPackageFileMap } from '@pnpm/store.pkg-finder'

const files = await readPackageFileMap(
  resolution,  // { integrity?, tarball?, directory?, type? }
  packageId,
  {
    storeDir: '/home/user/.local/share/pnpm/store/v10',
    lockfileDir: '/home/user/project',
    virtualStoreDirMaxLength: 120,
  }
)

if (files) {
  const manifestPath = files.get('package.json')
  const licensePath = files.get('LICENSE')
}
```

## Supported resolution types

- **Directory** (`type: 'directory'`): fetches the file list from the local directory.
- **Integrity** (`integrity` field): looks up the index file in CAFS by integrity hash.
- **Tarball** (`tarball` field): looks up the index file by package directory name.

Returns `undefined` for unsupported resolution types.

## Note on side effects

This function returns only the original package files. Files added or removed by post-install scripts (side effects) are not included. Use the raw `PackageFilesIndex` from `@pnpm/store.cafs` if you need side-effect files.

## License

MIT
