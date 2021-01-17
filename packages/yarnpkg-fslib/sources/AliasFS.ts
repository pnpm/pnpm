import {FakeFS}          from './FakeFS';
import {ProxiedFS}       from './ProxiedFS';
import {Path, PathUtils} from './path';

export type AliasFSOptions<P extends Path> = {
  baseFs: FakeFS<P>,
  pathUtils: PathUtils<P>,
};

export class AliasFS<P extends Path> extends ProxiedFS<P, P> {
  private readonly target: P;

  protected readonly baseFs: FakeFS<P>;

  constructor(target: P, {baseFs, pathUtils}: AliasFSOptions<P>) {
    super(pathUtils);

    this.target = target;
    this.baseFs = baseFs;
  }

  getRealPath() {
    return this.target;
  }

  getBaseFs() {
    return this.baseFs;
  }

  protected mapFromBase(p: P) {
    return p;
  }

  protected mapToBase(p: P) {
    return p;
  }
}
