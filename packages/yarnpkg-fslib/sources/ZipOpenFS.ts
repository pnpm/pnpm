import {Libzip}                                                                                                                                      from '@yarnpkg/libzip';
import {BigIntStats, constants, Stats}                                                                                                               from 'fs';

import {WatchOptions, WatchCallback, Watcher}                                                                                                        from './FakeFS';
import {FakeFS, MkdirOptions, RmdirOptions, WriteFileOptions, OpendirOptions}                                                                        from './FakeFS';
import {Dirent, SymlinkType}                                                                                                                         from './FakeFS';
import {CreateReadStreamOptions, CreateWriteStreamOptions, BasePortableFakeFS, ExtractHintOptions, WatchFileOptions, WatchFileCallback, StatWatcher} from './FakeFS';
import {NodeFS}                                                                                                                                      from './NodeFS';
import {ZipFS}                                                                                                                                       from './ZipFS';
import {watchFile, unwatchFile, unwatchAllFiles}                                                                                                     from './algorithms/watchFile';
import * as errors                                                                                                                                   from './errors';
import {Filename, FSPath, PortablePath}                                                                                                              from './path';

const ZIP_FD = 0x80000000;

const FILE_PARTS_REGEX = /.*?(?<!\/)\.zip(?=\/|$)/;

export type ZipOpenFSOptions = {
  baseFs?: FakeFS<PortablePath>,
  filter?: RegExp | null,
  libzip: Libzip,
  maxOpenFiles?: number,
  readOnlyArchives?: boolean,
  useCache?: boolean,
  /**
   * Maximum age in ms.
   * ZipFS instances are pruned from the cache if they aren't accessed within this amount of time.
   */
  maxAge?: number,
};

export class ZipOpenFS extends BasePortableFakeFS {
  static async openPromise<T>(fn: (zipOpenFs: ZipOpenFS) => Promise<T>, opts: ZipOpenFSOptions): Promise<T> {
    const zipOpenFs = new ZipOpenFS(opts);

    try {
      return await fn(zipOpenFs);
    } finally {
      zipOpenFs.saveAndClose();
    }
  }

  private readonly libzip: Libzip;

  private readonly baseFs: FakeFS<PortablePath>;

  private readonly zipInstances: Map<string, {zipFs: ZipFS, expiresAt: number, refCount: number}> | null;

  private readonly fdMap: Map<number, [ZipFS, number]> = new Map();
  private nextFd = 3;

  private readonly filter: RegExp | null;
  private readonly maxOpenFiles: number;
  private readonly maxAge: number;
  private readonly readOnlyArchives: boolean;

  private isZip: Set<PortablePath> = new Set();
  private notZip: Set<PortablePath> = new Set();
  private realPaths: Map<PortablePath, PortablePath> = new Map();

  constructor({libzip, baseFs = new NodeFS(), filter = null, maxOpenFiles = Infinity, readOnlyArchives = false, useCache = true, maxAge = 5000}: ZipOpenFSOptions) {
    super();

    this.libzip = libzip;

    this.baseFs = baseFs;

    this.zipInstances = useCache ? new Map() : null;

    this.filter = filter;
    this.maxOpenFiles = maxOpenFiles;
    this.readOnlyArchives = readOnlyArchives;
    this.maxAge = maxAge;
  }

  getExtractHint(hints: ExtractHintOptions) {
    return this.baseFs.getExtractHint(hints);
  }

  getRealPath() {
    return this.baseFs.getRealPath();
  }

  saveAndClose() {
    unwatchAllFiles(this);

    if (this.zipInstances) {
      for (const [path, {zipFs}] of this.zipInstances.entries()) {
        zipFs.saveAndClose();
        this.zipInstances.delete(path);
      }
    }
  }

  discardAndClose() {
    unwatchAllFiles(this);

    if (this.zipInstances) {
      for (const [path, {zipFs}] of this.zipInstances.entries()) {
        zipFs.discardAndClose();
        this.zipInstances.delete(path);
      }
    }
  }

  resolve(p: PortablePath) {
    return this.baseFs.resolve(p);
  }

  private remapFd(zipFs: ZipFS, fd: number) {
    const remappedFd = this.nextFd++ | ZIP_FD;
    this.fdMap.set(remappedFd, [zipFs, fd]);
    return remappedFd;
  }

  async openPromise(p: PortablePath, flags: string, mode?: number) {
    return await this.makeCallPromise(p, async () => {
      return await this.baseFs.openPromise(p, flags, mode);
    }, async (zipFs, {subPath}) => {
      return this.remapFd(zipFs, await zipFs.openPromise(subPath, flags, mode));
    });
  }

  openSync(p: PortablePath, flags: string, mode?: number) {
    return this.makeCallSync(p, () => {
      return this.baseFs.openSync(p, flags, mode);
    }, (zipFs, {subPath}) => {
      return this.remapFd(zipFs, zipFs.openSync(subPath, flags, mode));
    });
  }

  async opendirPromise(p: PortablePath, opts?: OpendirOptions) {
    return await this.makeCallPromise(p, async () => {
      return await this.baseFs.opendirPromise(p, opts);
    }, async (zipFs, {subPath}) => {
      return await zipFs.opendirPromise(subPath, opts);
    }, {
      requireSubpath: false,
    });
  }

  opendirSync(p: PortablePath, opts?: OpendirOptions) {
    return this.makeCallSync(p, () => {
      return this.baseFs.opendirSync(p, opts);
    }, (zipFs, {subPath}) => {
      return zipFs.opendirSync(subPath, opts);
    }, {
      requireSubpath: false,
    });
  }

  async readPromise(fd: number, buffer: Buffer, offset: number, length: number, position: number) {
    if ((fd & ZIP_FD) === 0)
      return await this.baseFs.readPromise(fd, buffer, offset, length, position);

    const entry = this.fdMap.get(fd);
    if (typeof entry === `undefined`)
      throw errors.EBADF(`read`);

    const [zipFs, realFd] = entry;
    return await zipFs.readPromise(realFd, buffer, offset, length, position);
  }

  readSync(fd: number, buffer: Buffer, offset: number, length: number, position: number) {
    if ((fd & ZIP_FD) === 0)
      return this.baseFs.readSync(fd, buffer, offset, length, position);

    const entry = this.fdMap.get(fd);
    if (typeof entry === `undefined`)
      throw errors.EBADF(`readSync`);

    const [zipFs, realFd] = entry;
    return zipFs.readSync(realFd, buffer, offset, length, position);
  }

  writePromise(fd: number, buffer: Buffer, offset?: number, length?: number, position?: number): Promise<number>;
  writePromise(fd: number, buffer: string, position?: number): Promise<number>;
  async writePromise(fd: number, buffer: Buffer | string, offset?: number, length?: number, position?: number): Promise<number> {
    if ((fd & ZIP_FD) === 0) {
      if (typeof buffer === `string`) {
        return await this.baseFs.writePromise(fd, buffer, offset);
      } else {
        return await this.baseFs.writePromise(fd, buffer, offset, length, position);
      }
    }

    const entry = this.fdMap.get(fd);
    if (typeof entry === `undefined`)
      throw errors.EBADF(`write`);

    const [zipFs, realFd] = entry;

    if (typeof buffer === `string`) {
      return await zipFs.writePromise(realFd, buffer, offset);
    } else {
      return await zipFs.writePromise(realFd, buffer, offset, length, position);
    }
  }

  writeSync(fd: number, buffer: Buffer, offset?: number, length?: number, position?: number): number;
  writeSync(fd: number, buffer: string, position?: number): number;
  writeSync(fd: number, buffer: Buffer | string, offset?: number, length?: number, position?: number): number {
    if ((fd & ZIP_FD) === 0) {
      if (typeof buffer === `string`) {
        return this.baseFs.writeSync(fd, buffer, offset);
      } else {
        return this.baseFs.writeSync(fd, buffer, offset, length, position);
      }
    }

    const entry = this.fdMap.get(fd);
    if (typeof entry === `undefined`)
      throw errors.EBADF(`writeSync`);

    const [zipFs, realFd] = entry;

    if (typeof buffer === `string`) {
      return zipFs.writeSync(realFd, buffer, offset);
    } else {
      return zipFs.writeSync(realFd, buffer, offset, length, position);
    }
  }

  async closePromise(fd: number) {
    if ((fd & ZIP_FD) === 0)
      return await this.baseFs.closePromise(fd);

    const entry = this.fdMap.get(fd);
    if (typeof entry === `undefined`)
      throw errors.EBADF(`close`);

    this.fdMap.delete(fd);

    const [zipFs, realFd] = entry;
    return await zipFs.closePromise(realFd);
  }

  closeSync(fd: number) {
    if ((fd & ZIP_FD) === 0)
      return this.baseFs.closeSync(fd);

    const entry = this.fdMap.get(fd);
    if (typeof entry === `undefined`)
      throw errors.EBADF(`closeSync`);

    this.fdMap.delete(fd);

    const [zipFs, realFd] = entry;
    return zipFs.closeSync(realFd);
  }

  createReadStream(p: PortablePath | null, opts?: CreateReadStreamOptions) {
    if (p === null)
      return this.baseFs.createReadStream(p, opts);

    return this.makeCallSync(p, () => {
      return this.baseFs.createReadStream(p, opts);
    }, (zipFs, {subPath}) => {
      return zipFs.createReadStream(subPath, opts);
    });
  }

  createWriteStream(p: PortablePath | null, opts?: CreateWriteStreamOptions) {
    if (p === null)
      return this.baseFs.createWriteStream(p, opts);

    return this.makeCallSync(p, () => {
      return this.baseFs.createWriteStream(p, opts);
    }, (zipFs, {subPath}) => {
      return zipFs.createWriteStream(subPath, opts);
    });
  }

  async realpathPromise(p: PortablePath) {
    return await this.makeCallPromise(p, async () => {
      return await this.baseFs.realpathPromise(p);
    }, async (zipFs, {archivePath, subPath}) => {
      let realArchivePath = this.realPaths.get(archivePath);
      if (typeof realArchivePath === `undefined`) {
        realArchivePath = await this.baseFs.realpathPromise(archivePath);
        this.realPaths.set(archivePath, realArchivePath);
      }

      return this.pathUtils.join(realArchivePath, this.pathUtils.relative(PortablePath.root, await zipFs.realpathPromise(subPath)));
    });
  }

  realpathSync(p: PortablePath) {
    return this.makeCallSync(p, () => {
      return this.baseFs.realpathSync(p);
    }, (zipFs, {archivePath, subPath}) => {
      let realArchivePath = this.realPaths.get(archivePath);
      if (typeof realArchivePath === `undefined`) {
        realArchivePath = this.baseFs.realpathSync(archivePath);
        this.realPaths.set(archivePath, realArchivePath);
      }

      return this.pathUtils.join(realArchivePath, this.pathUtils.relative(PortablePath.root, zipFs.realpathSync(subPath)));
    });
  }

  async existsPromise(p: PortablePath) {
    return await this.makeCallPromise(p, async () => {
      return await this.baseFs.existsPromise(p);
    }, async (zipFs, {subPath}) => {
      return await zipFs.existsPromise(subPath);
    });
  }

  existsSync(p: PortablePath) {
    return this.makeCallSync(p, () => {
      return this.baseFs.existsSync(p);
    }, (zipFs, {subPath}) => {
      return zipFs.existsSync(subPath);
    });
  }

  async accessPromise(p: PortablePath, mode?: number) {
    return await this.makeCallPromise(p, async () => {
      return await this.baseFs.accessPromise(p, mode);
    }, async (zipFs, {subPath}) => {
      return await zipFs.accessPromise(subPath, mode);
    });
  }

  accessSync(p: PortablePath, mode?: number) {
    return this.makeCallSync(p, () => {
      return this.baseFs.accessSync(p, mode);
    }, (zipFs, {subPath}) => {
      return zipFs.accessSync(subPath, mode);
    });
  }

  async statPromise(p: PortablePath): Promise<Stats>
  async statPromise(p: PortablePath, opts: {bigint: true}): Promise<BigIntStats>
  async statPromise(p: PortablePath, opts?: {bigint: boolean}): Promise<BigIntStats | Stats>
  async statPromise(p: PortablePath, opts?: { bigint: boolean }) {
    return await this.makeCallPromise(p, async () => {
      return await this.baseFs.statPromise(p, opts);
    }, async (zipFs, {subPath}) => {
      return await zipFs.statPromise(subPath, opts);
    });
  }

  statSync(p: PortablePath): Stats
  statSync(p: PortablePath, opts: {bigint: true}): BigIntStats
  statSync(p: PortablePath, opts?: {bigint: boolean}): BigIntStats | Stats
  statSync(p: PortablePath, opts?: { bigint: boolean }) {
    return this.makeCallSync(p, () => {
      return this.baseFs.statSync(p, opts);
    }, (zipFs, {subPath}) => {
      return zipFs.statSync(subPath, opts);
    });
  }

  async fstatPromise(fd: number): Promise<Stats>
  async fstatPromise(fd: number, opts: {bigint: true}): Promise<BigIntStats>
  async fstatPromise(fd: number, opts?: {bigint: boolean}): Promise<BigIntStats | Stats>
  async fstatPromise(fd: number, opts?: { bigint: boolean }) {
    if ((fd & ZIP_FD) === 0)
      return this.baseFs.fstatPromise(fd, opts);

    const entry = this.fdMap.get(fd);
    if (typeof entry === `undefined`)
      throw errors.EBADF(`fstat`);

    const [zipFs, realFd] = entry;
    return zipFs.fstatPromise(realFd, opts);
  }

  fstatSync(fd: number): Stats
  fstatSync(fd: number, opts: {bigint: true}): BigIntStats
  fstatSync(fd: number, opts?: {bigint: boolean}): BigIntStats | Stats
  fstatSync(fd: number, opts?: { bigint: boolean }) {
    if ((fd & ZIP_FD) === 0)
      return this.baseFs.fstatSync(fd, opts);

    const entry = this.fdMap.get(fd);
    if (typeof entry === `undefined`)
      throw errors.EBADF(`fstatSync`);

    const [zipFs, realFd] = entry;
    return zipFs.fstatSync(realFd, opts);
  }

  async lstatPromise(p: PortablePath): Promise<Stats>
  async lstatPromise(p: PortablePath, opts: {bigint: true}): Promise<BigIntStats>
  async lstatPromise(p: PortablePath, opts?: { bigint: boolean }): Promise<BigIntStats | Stats>
  async lstatPromise(p: PortablePath, opts?: { bigint: boolean }) {
    return await this.makeCallPromise(p, async () => {
      return await this.baseFs.lstatPromise(p, opts);
    }, async (zipFs, {subPath}) => {
      return await zipFs.lstatPromise(subPath, opts);
    });
  }

  lstatSync(p: PortablePath): Stats;
  lstatSync(p: PortablePath, opts: {bigint: true}): BigIntStats;
  lstatSync(p: PortablePath, opts?: { bigint: boolean }): BigIntStats | Stats
  lstatSync(p: PortablePath, opts?: { bigint: boolean }): BigIntStats | Stats {
    return this.makeCallSync(p, () => {
      return this.baseFs.lstatSync(p, opts);
    }, (zipFs, {subPath}) => {
      return zipFs.lstatSync(subPath, opts);
    });
  }

  async chmodPromise(p: PortablePath, mask: number) {
    return await this.makeCallPromise(p, async () => {
      return await this.baseFs.chmodPromise(p, mask);
    }, async (zipFs, {subPath}) => {
      return await zipFs.chmodPromise(subPath, mask);
    });
  }

  chmodSync(p: PortablePath, mask: number) {
    return this.makeCallSync(p, () => {
      return this.baseFs.chmodSync(p, mask);
    }, (zipFs, {subPath}) => {
      return zipFs.chmodSync(subPath, mask);
    });
  }

  async chownPromise(p: PortablePath, uid: number, gid: number) {
    return await this.makeCallPromise(p, async () => {
      return await this.baseFs.chownPromise(p, uid, gid);
    }, async (zipFs, {subPath}) => {
      return await zipFs.chownPromise(subPath, uid, gid);
    });
  }

  chownSync(p: PortablePath, uid: number, gid: number) {
    return this.makeCallSync(p, () => {
      return this.baseFs.chownSync(p, uid, gid);
    }, (zipFs, {subPath}) => {
      return zipFs.chownSync(subPath, uid, gid);
    });
  }

  async renamePromise(oldP: PortablePath, newP: PortablePath) {
    return await this.makeCallPromise(oldP, async () => {
      return await this.makeCallPromise(newP, async () => {
        return await this.baseFs.renamePromise(oldP, newP);
      }, async () => {
        throw Object.assign(new Error(`EEXDEV: cross-device link not permitted`), {code: `EEXDEV`});
      });
    }, async (zipFsO, {subPath: subPathO}) => {
      return await this.makeCallPromise(newP, async () => {
        throw Object.assign(new Error(`EEXDEV: cross-device link not permitted`), {code: `EEXDEV`});
      }, async (zipFsN, {subPath: subPathN}) => {
        if (zipFsO !== zipFsN) {
          throw Object.assign(new Error(`EEXDEV: cross-device link not permitted`), {code: `EEXDEV`});
        } else {
          return await zipFsO.renamePromise(subPathO, subPathN);
        }
      });
    });
  }

  renameSync(oldP: PortablePath, newP: PortablePath) {
    return this.makeCallSync(oldP, () => {
      return this.makeCallSync(newP, () => {
        return this.baseFs.renameSync(oldP, newP);
      }, async () => {
        throw Object.assign(new Error(`EEXDEV: cross-device link not permitted`), {code: `EEXDEV`});
      });
    }, (zipFsO, {subPath: subPathO}) => {
      return this.makeCallSync(newP, () => {
        throw Object.assign(new Error(`EEXDEV: cross-device link not permitted`), {code: `EEXDEV`});
      }, (zipFsN, {subPath: subPathN}) => {
        if (zipFsO !== zipFsN) {
          throw Object.assign(new Error(`EEXDEV: cross-device link not permitted`), {code: `EEXDEV`});
        } else {
          return zipFsO.renameSync(subPathO, subPathN);
        }
      });
    });
  }

  async copyFilePromise(sourceP: PortablePath, destP: PortablePath, flags: number = 0) {
    const fallback = async (sourceFs: FakeFS<PortablePath>, sourceP: PortablePath, destFs: FakeFS<PortablePath>, destP: PortablePath) => {
      if ((flags & constants.COPYFILE_FICLONE_FORCE) !== 0)
        throw Object.assign(new Error(`EXDEV: cross-device clone not permitted, copyfile '${sourceP}' -> ${destP}'`), {code: `EXDEV`});
      if ((flags & constants.COPYFILE_EXCL) && await this.existsPromise(sourceP))
        throw Object.assign(new Error(`EEXIST: file already exists, copyfile '${sourceP}' -> '${destP}'`), {code: `EEXIST`});

      let content;
      try {
        content = await sourceFs.readFilePromise(sourceP);
      } catch (error) {
        throw Object.assign(new Error(`EINVAL: invalid argument, copyfile '${sourceP}' -> '${destP}'`), {code: `EINVAL`});
      }

      await destFs.writeFilePromise(destP, content);
    };

    return await this.makeCallPromise(sourceP, async () => {
      return await this.makeCallPromise(destP, async () => {
        return await this.baseFs.copyFilePromise(sourceP, destP, flags);
      }, async (zipFsD, {subPath: subPathD}) => {
        return await fallback(this.baseFs, sourceP, zipFsD, subPathD);
      });
    }, async (zipFsS, {subPath: subPathS}) => {
      return await this.makeCallPromise(destP, async () => {
        return await fallback(zipFsS, subPathS, this.baseFs, destP);
      }, async (zipFsD, {subPath: subPathD}) => {
        if (zipFsS !== zipFsD) {
          return await fallback(zipFsS, subPathS, zipFsD, subPathD);
        } else {
          return await zipFsS.copyFilePromise(subPathS, subPathD, flags);
        }
      });
    });
  }

  copyFileSync(sourceP: PortablePath, destP: PortablePath, flags: number = 0) {
    const fallback = (sourceFs: FakeFS<PortablePath>, sourceP: PortablePath, destFs: FakeFS<PortablePath>, destP: PortablePath) => {
      if ((flags & constants.COPYFILE_FICLONE_FORCE) !== 0)
        throw Object.assign(new Error(`EXDEV: cross-device clone not permitted, copyfile '${sourceP}' -> ${destP}'`), {code: `EXDEV`});
      if ((flags & constants.COPYFILE_EXCL) && this.existsSync(sourceP))
        throw Object.assign(new Error(`EEXIST: file already exists, copyfile '${sourceP}' -> '${destP}'`), {code: `EEXIST`});

      let content;
      try {
        content = sourceFs.readFileSync(sourceP);
      } catch (error) {
        throw Object.assign(new Error(`EINVAL: invalid argument, copyfile '${sourceP}' -> '${destP}'`), {code: `EINVAL`});
      }

      destFs.writeFileSync(destP, content);
    };

    return this.makeCallSync(sourceP, () => {
      return this.makeCallSync(destP, () => {
        return this.baseFs.copyFileSync(sourceP, destP, flags);
      }, (zipFsD, {subPath: subPathD}) => {
        return fallback(this.baseFs, sourceP, zipFsD, subPathD);
      });
    }, (zipFsS, {subPath: subPathS}) => {
      return this.makeCallSync(destP, () => {
        return fallback(zipFsS, subPathS, this.baseFs, destP);
      }, (zipFsD, {subPath: subPathD}) => {
        if (zipFsS !== zipFsD) {
          return fallback(zipFsS, subPathS, zipFsD, subPathD);
        } else {
          return zipFsS.copyFileSync(subPathS, subPathD, flags);
        }
      });
    });
  }

  async appendFilePromise(p: FSPath<PortablePath>, content: string | Buffer | ArrayBuffer | DataView, opts?: WriteFileOptions) {
    return await this.makeCallPromise(p, async () => {
      return await this.baseFs.appendFilePromise(p, content, opts);
    }, async (zipFs, {subPath}) => {
      return await zipFs.appendFilePromise(subPath, content, opts);
    });
  }

  appendFileSync(p: FSPath<PortablePath>, content: string | Buffer | ArrayBuffer | DataView, opts?: WriteFileOptions) {
    return this.makeCallSync(p, () => {
      return this.baseFs.appendFileSync(p, content, opts);
    }, (zipFs, {subPath}) => {
      return zipFs.appendFileSync(subPath, content, opts);
    });
  }

  async writeFilePromise(p: FSPath<PortablePath>, content: string | Buffer | ArrayBuffer | DataView, opts?: WriteFileOptions) {
    return await this.makeCallPromise(p, async () => {
      return await this.baseFs.writeFilePromise(p, content, opts);
    }, async (zipFs, {subPath}) => {
      return await zipFs.writeFilePromise(subPath, content, opts);
    });
  }

  writeFileSync(p: FSPath<PortablePath>, content: string | Buffer | ArrayBuffer | DataView, opts?: WriteFileOptions) {
    return this.makeCallSync(p, () => {
      return this.baseFs.writeFileSync(p, content, opts);
    }, (zipFs, {subPath}) => {
      return zipFs.writeFileSync(subPath, content, opts);
    });
  }

  async unlinkPromise(p: PortablePath) {
    return await this.makeCallPromise(p, async () => {
      return await this.baseFs.unlinkPromise(p);
    }, async (zipFs, {subPath}) => {
      return await zipFs.unlinkPromise(subPath);
    });
  }

  unlinkSync(p: PortablePath) {
    return this.makeCallSync(p, () => {
      return this.baseFs.unlinkSync(p);
    }, (zipFs, {subPath}) => {
      return zipFs.unlinkSync(subPath);
    });
  }

  async utimesPromise(p: PortablePath, atime: Date | string | number, mtime: Date | string | number) {
    return await this.makeCallPromise(p, async () => {
      return await this.baseFs.utimesPromise(p, atime, mtime);
    }, async (zipFs, {subPath}) => {
      return await zipFs.utimesPromise(subPath, atime, mtime);
    });
  }

  utimesSync(p: PortablePath, atime: Date | string | number, mtime: Date | string | number) {
    return this.makeCallSync(p, () => {
      return this.baseFs.utimesSync(p, atime, mtime);
    }, (zipFs, {subPath}) => {
      return zipFs.utimesSync(subPath, atime, mtime);
    });
  }

  async mkdirPromise(p: PortablePath, opts?: MkdirOptions) {
    return await this.makeCallPromise(p, async () => {
      return await this.baseFs.mkdirPromise(p, opts);
    }, async (zipFs, {subPath}) => {
      return await zipFs.mkdirPromise(subPath, opts);
    });
  }

  mkdirSync(p: PortablePath, opts?: MkdirOptions) {
    return this.makeCallSync(p, () => {
      return this.baseFs.mkdirSync(p, opts);
    }, (zipFs, {subPath}) => {
      return zipFs.mkdirSync(subPath, opts);
    });
  }

  async rmdirPromise(p: PortablePath, opts?: RmdirOptions) {
    return await this.makeCallPromise(p, async () => {
      return await this.baseFs.rmdirPromise(p, opts);
    }, async (zipFs, {subPath}) => {
      return await zipFs.rmdirPromise(subPath, opts);
    });
  }

  rmdirSync(p: PortablePath, opts?: RmdirOptions) {
    return this.makeCallSync(p, () => {
      return this.baseFs.rmdirSync(p, opts);
    }, (zipFs, {subPath}) => {
      return zipFs.rmdirSync(subPath, opts);
    });
  }

  async linkPromise(existingP: PortablePath, newP: PortablePath) {
    return await this.makeCallPromise(newP, async () => {
      return await this.baseFs.linkPromise(existingP, newP);
    }, async (zipFs, {subPath}) => {
      return await zipFs.linkPromise(existingP, subPath);
    });
  }

  linkSync(existingP: PortablePath, newP: PortablePath) {
    return this.makeCallSync(newP, () => {
      return this.baseFs.linkSync(existingP, newP);
    }, (zipFs, {subPath}) => {
      return zipFs.linkSync(existingP, subPath);
    });
  }

  async symlinkPromise(target: PortablePath, p: PortablePath, type?: SymlinkType) {
    return await this.makeCallPromise(p, async () => {
      return await this.baseFs.symlinkPromise(target, p, type);
    }, async (zipFs, {subPath}) => {
      return await zipFs.symlinkPromise(target, subPath);
    });
  }

  symlinkSync(target: PortablePath, p: PortablePath, type?: SymlinkType) {
    return this.makeCallSync(p, () => {
      return this.baseFs.symlinkSync(target, p, type);
    }, (zipFs, {subPath}) => {
      return zipFs.symlinkSync(target, subPath);
    });
  }

  readFilePromise(p: FSPath<PortablePath>, encoding: 'utf8'): Promise<string>;
  readFilePromise(p: FSPath<PortablePath>, encoding?: string): Promise<Buffer>;
  async readFilePromise(p: FSPath<PortablePath>, encoding?: string) {
    return this.makeCallPromise(p, async () => {
      // This weird switch is required to tell TypeScript that the signatures are proper (otherwise it thinks that only the generic one is covered)
      switch (encoding) {
        case `utf8`:
          return await this.baseFs.readFilePromise(p, encoding);
        default:
          return await this.baseFs.readFilePromise(p, encoding);
      }
    }, async (zipFs, {subPath}) => {
      return await zipFs.readFilePromise(subPath, encoding);
    });
  }

  readFileSync(p: FSPath<PortablePath>, encoding: 'utf8'): string;
  readFileSync(p: FSPath<PortablePath>, encoding?: string): Buffer;
  readFileSync(p: FSPath<PortablePath>, encoding?: string) {
    return this.makeCallSync(p, () => {
      // This weird switch is required to tell TypeScript that the signatures are proper (otherwise it thinks that only the generic one is covered)
      switch (encoding) {
        case `utf8`:
          return this.baseFs.readFileSync(p, encoding);
        default:
          return this.baseFs.readFileSync(p, encoding);
      }
    }, (zipFs, {subPath}) => {
      return zipFs.readFileSync(subPath, encoding);
    });
  }

  async readdirPromise(p: PortablePath): Promise<Array<Filename>>;
  async readdirPromise(p: PortablePath, opts: {withFileTypes: false}): Promise<Array<Filename>>;
  async readdirPromise(p: PortablePath, opts: {withFileTypes: true}): Promise<Array<Dirent>>;
  async readdirPromise(p: PortablePath, opts: {withFileTypes: boolean}): Promise<Array<Filename> | Array<Dirent>>;
  async readdirPromise(p: PortablePath, {withFileTypes}: {withFileTypes?: boolean} = {}): Promise<Array<string> | Array<Dirent>> {
    return await this.makeCallPromise(p, async () => {
      return await this.baseFs.readdirPromise(p, {withFileTypes: withFileTypes as any});
    }, async (zipFs, {subPath}) => {
      return await zipFs.readdirPromise(subPath, {withFileTypes: withFileTypes as any});
    }, {
      requireSubpath: false,
    });
  }

  readdirSync(p: PortablePath): Array<Filename>;
  readdirSync(p: PortablePath, opts: {withFileTypes: false}): Array<Filename>;
  readdirSync(p: PortablePath, opts: {withFileTypes: true}): Array<Dirent>;
  readdirSync(p: PortablePath, opts: {withFileTypes: boolean}): Array<Filename> | Array<Dirent>;
  readdirSync(p: PortablePath, {withFileTypes}: {withFileTypes?: boolean} = {}): Array<string> | Array<Dirent> {
    return this.makeCallSync(p, () => {
      return this.baseFs.readdirSync(p, {withFileTypes: withFileTypes as any});
    }, (zipFs, {subPath}) => {
      return zipFs.readdirSync(subPath, {withFileTypes: withFileTypes as any});
    }, {
      requireSubpath: false,
    });
  }

  async readlinkPromise(p: PortablePath) {
    return await this.makeCallPromise(p, async () => {
      return await this.baseFs.readlinkPromise(p);
    }, async (zipFs, {subPath}) => {
      return await zipFs.readlinkPromise(subPath);
    });
  }

  readlinkSync(p: PortablePath) {
    return this.makeCallSync(p, () => {
      return this.baseFs.readlinkSync(p);
    }, (zipFs, {subPath}) => {
      return zipFs.readlinkSync(subPath);
    });
  }

  async truncatePromise(p: PortablePath, len?: number) {
    return await this.makeCallPromise(p, async () => {
      return await this.baseFs.truncatePromise(p, len);
    }, async (zipFs, {subPath}) => {
      return await zipFs.truncatePromise(subPath, len);
    });
  }

  truncateSync(p: PortablePath, len?: number) {
    return this.makeCallSync(p, () => {
      return this.baseFs.truncateSync(p, len);
    }, (zipFs, {subPath}) => {
      return zipFs.truncateSync(subPath, len);
    });
  }

  watch(p: PortablePath, cb?: WatchCallback): Watcher;
  watch(p: PortablePath, opts: WatchOptions, cb?: WatchCallback): Watcher;
  watch(p: PortablePath, a?: WatchOptions | WatchCallback, b?: WatchCallback) {
    return this.makeCallSync(p, () => {
      return this.baseFs.watch(
        p,
        // @ts-expect-error
        a,
        b,
      );
    }, (zipFs, {subPath}) => {
      return zipFs.watch(
        subPath,
        // @ts-expect-error
        a,
        b,
      );
    });
  }

  watchFile(p: PortablePath, cb: WatchFileCallback): StatWatcher;
  watchFile(p: PortablePath, opts: WatchFileOptions, cb: WatchFileCallback): StatWatcher;
  watchFile(p: PortablePath, a: WatchFileOptions | WatchFileCallback, b?: WatchFileCallback) {
    return this.makeCallSync(p, () => {
      return this.baseFs.watchFile(
        p,
        // @ts-expect-error
        a,
        b,
      );
    }, () => {
      return watchFile(this, p, a, b);
    });
  }

  unwatchFile(p: PortablePath, cb?: WatchFileCallback): void {
    return this.makeCallSync(p, () => {
      return this.baseFs.unwatchFile(p, cb);
    }, () => {
      return unwatchFile(this, p, cb);
    });
  }

  private async makeCallPromise<T>(p: FSPath<PortablePath>, discard: () => Promise<T>, accept: (zipFS: ZipFS, zipInfo: {archivePath: PortablePath, subPath: PortablePath}) => Promise<T>, {requireSubpath = true}: {requireSubpath?: boolean} = {}): Promise<T> {
    if (typeof p !== `string`)
      return await discard();

    const normalizedP = this.resolve(p);

    const zipInfo = this.findZip(normalizedP);
    if (!zipInfo)
      return await discard();

    if (requireSubpath && zipInfo.subPath === `/`)
      return await discard();

    return await this.getZipPromise(zipInfo.archivePath, async zipFs => await accept(zipFs, zipInfo));
  }

  private makeCallSync<T>(p: FSPath<PortablePath>, discard: () => T, accept: (zipFS: ZipFS, zipInfo: {archivePath: PortablePath, subPath: PortablePath}) => T, {requireSubpath = true}: {requireSubpath?: boolean} = {}): T {
    if (typeof p !== `string`)
      return discard();

    const normalizedP = this.resolve(p);

    const zipInfo = this.findZip(normalizedP);
    if (!zipInfo)
      return discard();

    if (requireSubpath && zipInfo.subPath === `/`)
      return discard();

    return this.getZipSync(zipInfo.archivePath, zipFs => accept(zipFs, zipInfo));
  }

  private findZip(p: PortablePath) {
    if (this.filter && !this.filter.test(p))
      return null;

    let filePath = `` as PortablePath;

    while (true) {
      const parts = FILE_PARTS_REGEX.exec(p.substr(filePath.length));
      if (!parts)
        return null;

      filePath = this.pathUtils.join(filePath, parts[0] as PortablePath);

      if (this.isZip.has(filePath) === false) {
        if (this.notZip.has(filePath))
          continue;

        try {
          if (!this.baseFs.lstatSync(filePath).isFile()) {
            this.notZip.add(filePath);
            continue;
          }
        } catch {
          return null;
        }

        this.isZip.add(filePath);
      }

      return {
        archivePath: filePath,
        subPath: this.pathUtils.join(PortablePath.root, p.substr(filePath.length) as PortablePath),
      };
    }
  }

  private limitOpenFilesTimeout: NodeJS.Timeout | null = null;
  private limitOpenFiles(max: number | null) {
    if (this.zipInstances === null)
      return;

    const now = Date.now();
    let nextExpiresAt = now + this.maxAge;
    let closeCount = max === null ? 0 : this.zipInstances.size - max;

    for (const [path, {zipFs, expiresAt, refCount}] of this.zipInstances.entries()) {
      if (refCount !== 0 || zipFs.hasOpenFileHandles()) {
        continue;
      } else if (now >= expiresAt) {
        zipFs.saveAndClose();
        this.zipInstances.delete(path);
        closeCount -= 1;
        continue;
      } else if (max === null || closeCount <= 0) {
        nextExpiresAt = expiresAt;
        break;
      }

      zipFs.saveAndClose();
      this.zipInstances.delete(path);
      closeCount -= 1;
    }

    if (this.limitOpenFilesTimeout === null && ((max === null && this.zipInstances.size > 0) || max !== null)) {
      this.limitOpenFilesTimeout = setTimeout(() => {
        this.limitOpenFilesTimeout = null;
        this.limitOpenFiles(null);
      }, nextExpiresAt - now).unref();
    }
  }

  private async getZipPromise<T>(p: PortablePath, accept: (zipFs: ZipFS) => Promise<T>) {
    const getZipOptions = async () => ({
      baseFs: this.baseFs,
      libzip: this.libzip,
      readOnly: this.readOnlyArchives,
      stats: await this.baseFs.statPromise(p),
    });

    if (this.zipInstances) {
      let cachedZipFs = this.zipInstances.get(p);

      if (!cachedZipFs) {
        const zipOptions = await getZipOptions();

        // We need to recheck because concurrent getZipPromise calls may
        // have instantiated the zip archive while we were waiting
        cachedZipFs = this.zipInstances.get(p);
        if (!cachedZipFs) {
          cachedZipFs = {
            zipFs: new ZipFS(p, zipOptions),
            expiresAt: 0,
            refCount: 0,
          };
        }
      }

      // Removing then re-adding the field allows us to easily implement
      // a basic LRU garbage collection strategy
      this.zipInstances.delete(p);
      this.limitOpenFiles(this.maxOpenFiles - 1);
      this.zipInstances.set(p, cachedZipFs);

      cachedZipFs.expiresAt = Date.now() + this.maxAge;
      cachedZipFs.refCount += 1;
      try {
        return await accept(cachedZipFs.zipFs);
      } finally {
        cachedZipFs.refCount -= 1;
      }
    } else {
      const zipFs = new ZipFS(p, await getZipOptions());

      try {
        return await accept(zipFs);
      } finally {
        zipFs.saveAndClose();
      }
    }
  }

  private getZipSync<T>(p: PortablePath, accept: (zipFs: ZipFS) => T) {
    const getZipOptions = () => ({
      baseFs: this.baseFs,
      libzip: this.libzip,
      readOnly: this.readOnlyArchives,
      stats: this.baseFs.statSync(p),
    });

    if (this.zipInstances) {
      let cachedZipFs = this.zipInstances.get(p);

      if (!cachedZipFs) {
        cachedZipFs = {
          zipFs: new ZipFS(p, getZipOptions()),
          expiresAt: 0,
          refCount: 0,
        };
      }

      // Removing then re-adding the field allows us to easily implement
      // a basic LRU garbage collection strategy
      this.zipInstances.delete(p);
      this.limitOpenFiles(this.maxOpenFiles - 1);
      this.zipInstances.set(p, cachedZipFs);

      cachedZipFs.expiresAt = Date.now() + this.maxAge;
      return accept(cachedZipFs.zipFs);
    } else {
      const zipFs = new ZipFS(p, getZipOptions());

      try {
        return accept(zipFs);
      } finally {
        zipFs.saveAndClose();
      }
    }
  }
}
