import os                                     from 'os';

import {NodeFS}                               from './NodeFS';
import {Filename, PortablePath, npath, ppath} from './path';

function getTempName(prefix: string) {
  const tmpdir = npath.toPortablePath(os.tmpdir());
  const hash = Math.ceil(Math.random() * 0x100000000).toString(16).padStart(8, `0`);

  return ppath.join(tmpdir, `${prefix}${hash}` as Filename);
}

export type XFS = NodeFS & {
  detachTemp(p: PortablePath): void;

  mktempSync(): PortablePath;
  mktempSync<T>(cb: (p: PortablePath) => T): T;

  mktempPromise(): Promise<PortablePath>;
  mktempPromise<T>(cb: (p: PortablePath) => Promise<T>): Promise<T>;

  /**
   * Tries to remove all temp folders created by mktempSync and mktempPromise
   */
  rmtempPromise(): Promise<void>;

  /**
   * Tries to remove all temp folders created by mktempSync and mktempPromise
   */
  rmtempSync(): void;
};

const tmpdirs = new Set<PortablePath>();

let cleanExitRegistered = false;

function registerCleanExit() {
  if (cleanExitRegistered)
    return;

  cleanExitRegistered = true;
  process.once(`exit`, () => {
    xfs.rmtempSync();
  });
}

export const xfs: XFS = Object.assign(new NodeFS(), {
  detachTemp(p: PortablePath) {
    tmpdirs.delete(p);
  },

  mktempSync<T>(this: XFS, cb?: (p: PortablePath) => T) {
    registerCleanExit();

    while (true) {
      const p = getTempName(`xfs-`);

      try {
        this.mkdirSync(p);
      } catch (error) {
        if (error.code === `EEXIST`) {
          continue;
        } else {
          throw error;
        }
      }

      const realP = this.realpathSync(p);
      tmpdirs.add(realP);

      if (typeof cb !== `undefined`) {
        try {
          return cb(realP);
        } finally {
          if (tmpdirs.has(realP)) {
            tmpdirs.delete(realP);
            try {
              this.removeSync(realP);
            } catch {
              // Too bad if there's an error
            }
          }
        }
      } else {
        return p;
      }
    }
  },

  async mktempPromise<T>(this: XFS, cb?: (p: PortablePath) => Promise<T>) {
    registerCleanExit();

    while (true) {
      const p = getTempName(`xfs-`);

      try {
        await this.mkdirPromise(p);
      } catch (error) {
        if (error.code === `EEXIST`) {
          continue;
        } else {
          throw error;
        }
      }

      const realP = await this.realpathPromise(p);
      tmpdirs.add(realP);

      if (typeof cb !== `undefined`) {
        try {
          return await cb(realP);
        } finally {
          if (tmpdirs.has(realP)) {
            tmpdirs.delete(realP);
            try {
              await this.removePromise(realP);
            } catch {
              // Too bad if there's an error
            }
          }
        }
      } else {
        return realP;
      }
    }
  },

  async rmtempPromise() {
    await Promise.all(Array.from(tmpdirs.values()).map(async p => {
      try {
        await xfs.removePromise(p, {maxRetries: 0});
        tmpdirs.delete(p);
      } catch {
        // Too bad if there's an error
      }
    }));
  },

  rmtempSync() {
    for (const p of tmpdirs) {
      try {
        xfs.removeSync(p);
        tmpdirs.delete(p);
      } catch {
        // Too bad if there's an error
      }
    }
  },
});
