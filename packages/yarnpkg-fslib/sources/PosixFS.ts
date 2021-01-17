import {FakeFS}                          from './FakeFS';
import {ProxiedFS}                       from './ProxiedFS';
import {npath, NativePath, PortablePath} from './path';

export class PosixFS extends ProxiedFS<NativePath, PortablePath> {
  protected readonly baseFs: FakeFS<PortablePath>;

  constructor(baseFs: FakeFS<PortablePath>) {
    super(npath);

    this.baseFs = baseFs;
  }

  protected mapFromBase(path: PortablePath) {
    return npath.fromPortablePath(path);
  }

  protected mapToBase(path: NativePath) {
    return npath.toPortablePath(path);
  }
}
