import {FakeFS}          from './FakeFS';
import {ProxiedFS}       from './ProxiedFS';
import {Path, PathUtils} from './path';

export type LazyFSFactory<P extends Path> = () => FakeFS<P>;

export class LazyFS<P extends Path> extends ProxiedFS<P, P> {
  private readonly factory: LazyFSFactory<P>;

  private instance: FakeFS<P> | null = null;

  constructor(factory: LazyFSFactory<P>, pathUtils: PathUtils<P>) {
    super(pathUtils);

    this.factory = factory;
  }

  protected get baseFs() {
    if (!this.instance)
      this.instance = this.factory();

    return this.instance;
  }

  protected set baseFs(value) {
    this.instance = value;
  }

  protected mapFromBase(p: P) {
    return p;
  }

  protected mapToBase(p: P) {
    return p;
  }
}
