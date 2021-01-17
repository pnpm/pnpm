import {FakeFS}              from './FakeFS';
import {NodeFS}              from './NodeFS';
import {ProxiedFS}           from './ProxiedFS';
import {PortablePath, ppath} from './path';

export type CwdFSOptions = {
  baseFs?: FakeFS<PortablePath>,
};

export class CwdFS extends ProxiedFS<PortablePath, PortablePath> {
  private readonly target: PortablePath;

  protected readonly baseFs: FakeFS<PortablePath>;

  constructor(target: PortablePath, {baseFs = new NodeFS()}: CwdFSOptions = {}) {
    super(ppath);

    this.target = this.pathUtils.normalize(target);

    this.baseFs = baseFs;
  }

  getRealPath() {
    return this.pathUtils.resolve(this.baseFs.getRealPath(), this.target);
  }

  resolve(p: PortablePath) {
    if (this.pathUtils.isAbsolute(p)) {
      return ppath.normalize(p);
    } else {
      return this.baseFs.resolve(ppath.join(this.target, p));
    }
  }

  mapFromBase(path: PortablePath) {
    return path;
  }

  mapToBase(path: PortablePath) {
    if (this.pathUtils.isAbsolute(path)) {
      return path;
    } else {
      return this.pathUtils.join(this.target, path);
    }
  }
}
