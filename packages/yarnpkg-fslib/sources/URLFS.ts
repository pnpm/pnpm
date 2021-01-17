import {URL, fileURLToPath} from 'url';

import {FakeFS}             from './FakeFS';
import {ProxiedFS}          from './ProxiedFS';
import {npath, NativePath}  from './path';

/**
 * Adds support for file URLs to the wrapped `baseFs`, but *not* inside the typings.
 *
 * Only exists for compatibility with Node's behavior.
 *
 * Automatically wraps all FS instances passed to `patchFs` & `extendFs`.
 *
 * Don't use it!
 */
export class URLFS extends ProxiedFS<NativePath, NativePath> {
  protected readonly baseFs: FakeFS<NativePath>;

  constructor(baseFs: FakeFS<NativePath>) {
    super(npath);

    this.baseFs = baseFs;
  }

  protected mapFromBase(path: NativePath) {
    return path;
  }

  protected mapToBase(path: URL | NativePath) {
    if (path instanceof URL)
      return fileURLToPath(path);

    return path;
  }
}
