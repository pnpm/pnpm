import {NoParamCallback}     from 'fs';

import {Dir, Dirent, FakeFS} from '../FakeFS';
import * as errors           from '../errors';
import {Filename, Path}      from '../path';

export type CustomDirOptions = {
  onClose?: () => void;
};

export class CustomDir<P extends Path> implements Dir<P> {
  constructor(
    public readonly path: P,
    private readonly nextDirent: () => Dirent | null,
    private readonly opts: CustomDirOptions = {},
  ) {}

  public closed: boolean = false;

  throwIfClosed() {
    if (this.closed) {
      throw errors.ERR_DIR_CLOSED();
    }
  }

  async * [Symbol.asyncIterator]() {
    try {
      let dirent: Dirent | null;
      // eslint-disable-next-line no-cond-assign
      while ((dirent = await this.read()) !== null) {
        yield dirent;
      }
    } finally {
      await this.close();
    }
  }

  read(): Promise<Dirent>;
  read(cb: (err: NodeJS.ErrnoException | null, dirent: Dirent | null) => void): void;
  read(cb?: (err: NodeJS.ErrnoException | null, dirent: Dirent | null) => void) {
    const dirent = this.readSync();

    if (typeof cb !== `undefined`)
      return cb(null, dirent);

    return Promise.resolve(dirent);
  }

  readSync() {
    this.throwIfClosed();

    return this.nextDirent();
  }

  close(): Promise<void>;
  close(cb: NoParamCallback): void;
  close(cb?: NoParamCallback) {
    this.closeSync();

    if (typeof cb !== `undefined`)
      return cb(null);

    return Promise.resolve();
  }

  closeSync() {
    this.throwIfClosed();

    this.opts.onClose?.();
    this.closed = true;
  }
}

export function opendir<P extends Path>(fakeFs: FakeFS<P>, path: P, entries: Array<Filename>, opts?: CustomDirOptions) {
  const nextDirent = () => {
    const filename = entries.shift();
    if (typeof filename === `undefined`)
      return null;

    return Object.assign(fakeFs.statSync(fakeFs.pathUtils.join(path, filename)), {
      name: filename,
    });
  };

  return new CustomDir(path, nextDirent, opts);
}
