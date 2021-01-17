import {BigIntStats, Stats}                from 'fs';
import * as nodeUtils                      from 'util';

import {S_IFDIR, S_IFLNK, S_IFMT, S_IFREG} from './constants';
import {Filename}                          from './path';

export class DirEntry {
  public name: Filename = `` as Filename;
  public mode: number = 0;

  isBlockDevice() {
    return false;
  }

  isCharacterDevice() {
    return false;
  }

  isDirectory() {
    return (this.mode & S_IFMT) === S_IFDIR;
  }

  isFIFO() {
    return false;
  }

  isFile() {
    return (this.mode & S_IFMT) === S_IFREG;
  }

  isSocket() {
    return false;
  }

  isSymbolicLink() {
    return (this.mode & S_IFMT) === S_IFLNK;
  }
}

export class StatEntry {
  uid = 0;
  gid = 0;

  size = 0;
  blksize = 0;

  atimeMs = 0;
  mtimeMs = 0;
  ctimeMs = 0;
  birthtimeMs = 0;

  atime = new Date(0);
  mtime = new Date(0);
  ctime = new Date(0);
  birthtime = new Date(0);

  dev = 0;
  ino = 0;
  mode = S_IFREG | 0o644;
  nlink = 1;
  rdev = 0;
  blocks = 1;

  isBlockDevice() {
    return false;
  }

  isCharacterDevice() {
    return false;
  }

  isDirectory() {
    return (this.mode & S_IFMT) === S_IFDIR;
  }

  isFIFO() {
    return false;
  }

  isFile() {
    return (this.mode & S_IFMT) === S_IFREG;
  }

  isSocket() {
    return false;
  }

  isSymbolicLink() {
    return (this.mode & S_IFMT) === S_IFLNK;
  }
}

export class BigIntStatsEntry {
  uid = BigInt(0);
  gid = BigInt(0);

  size = BigInt(0);
  blksize = BigInt(0);

  atimeMs = BigInt(0);
  mtimeMs = BigInt(0);
  ctimeMs = BigInt(0);
  birthtimeMs = BigInt(0);

  atimeNs = BigInt(0);
  mtimeNs = BigInt(0);
  ctimeNs = BigInt(0);
  birthtimeNs= BigInt(0);

  atime = new Date(0);
  mtime = new Date(0);
  ctime = new Date(0);
  birthtime = new Date(0);

  dev = BigInt(0);
  ino = BigInt(0);
  mode = BigInt(S_IFREG | 0o644);
  nlink = BigInt(1);
  rdev = BigInt(0);
  blocks = BigInt(1);

  isBlockDevice() {
    return false;
  }

  isCharacterDevice() {
    return false;
  }

  isDirectory() {
    return (this.mode & BigInt(S_IFMT)) === BigInt(S_IFDIR);
  }

  isFIFO() {
    return false;
  }

  isFile() {
    return (this.mode & BigInt(S_IFMT)) === BigInt(S_IFREG);
  }

  isSocket() {
    return false;
  }

  isSymbolicLink() {
    return (this.mode & BigInt(S_IFMT)) === BigInt(S_IFLNK);
  }
}

export function makeDefaultStats() {
  return new StatEntry();
}

export function makeEmptyStats() {
  return clearStats(makeDefaultStats());
}

/**
 * Mutates the provided stats object to zero it out then returns it for convenience
 */
export function clearStats(stats: Stats | BigIntStats) {
  for (const key in stats) {
    if (Object.prototype.hasOwnProperty.call(stats, key)) {
      const element = stats[key as keyof typeof stats];
      if (typeof element === `number`) {
        // @ts-expect-error Typescript can't tell that stats[key] is a number
        stats[key as keyof typeof stats] = 0;
      } else if (typeof element === `bigint`) {
        // @ts-expect-error Typescript can't tell that stats[key] is a bigint
        stats[key as keyof typeof stats] = BigInt(0);
      } else if (nodeUtils.types.isDate(element)) {
        // @ts-expect-error Typescript can't tell that stats[key] is a bigint
        stats[key as keyof typeof stats] = new Date(0);
      }
    }
  }

  return stats;
}

export function convertToBigIntStats(stats: Stats): BigIntStats {
  const bigintStats = new BigIntStatsEntry();

  for (const key in stats) {
    if (Object.prototype.hasOwnProperty.call(stats, key)) {
      const element = stats[key as keyof typeof stats];
      if (typeof element === `number`) {
        // @ts-expect-error Typescript isn't able to tell this is valid
        bigintStats[key as keyof typeof bigintStats] = BigInt(element);
      } else if (nodeUtils.types.isDate(element)) {
        // @ts-expect-error Typescript isn't able to tell this is valid
        bigintStats[key as keyof typeof bigintStats] = new Date(element);
      }
    }
  }

  bigintStats.atimeNs = bigintStats.atimeMs * BigInt(1e6);
  bigintStats.mtimeNs = bigintStats.mtimeMs * BigInt(1e6);
  bigintStats.ctimeNs = bigintStats.ctimeMs * BigInt(1e6);
  bigintStats.birthtimeNs = bigintStats.birthtimeMs * BigInt(1e6);

  return bigintStats as unknown as BigIntStats;
}

export function areStatsEqual(a: Stats | BigIntStatsEntry, b: Stats | BigIntStatsEntry): boolean {
  if (a.atimeMs !== b.atimeMs)
    return false;

  if (a.birthtimeMs !== b.birthtimeMs)
    return false;

  if (a.blksize !== b.blksize)
    return false;

  if (a.blocks !== b.blocks)
    return false;

  if (a.ctimeMs !== b.ctimeMs)
    return false;

  if (a.dev !== b.dev)
    return false;

  if (a.gid !== b.gid)
    return false;

  if (a.ino !== b.ino)
    return false;

  if (a.isBlockDevice() !== b.isBlockDevice())
    return false;

  if (a.isCharacterDevice() !== b.isCharacterDevice())
    return false;

  if (a.isDirectory() !== b.isDirectory())
    return false;

  if (a.isFIFO() !== b.isFIFO())
    return false;

  if (a.isFile() !== b.isFile())
    return false;

  if (a.isSocket() !== b.isSocket())
    return false;

  if (a.isSymbolicLink() !== b.isSymbolicLink())
    return false;

  if (a.mode !== b.mode)
    return false;

  if (a.mtimeMs !== b.mtimeMs)
    return false;

  if (a.nlink !== b.nlink)
    return false;

  if (a.rdev !== b.rdev)
    return false;

  if (a.size !== b.size)
    return false;

  if (a.uid !== b.uid)
    return false;

  const aN = a as BigIntStatsEntry;
  const bN = b as BigIntStatsEntry;

  if (aN.atimeNs !== bN.atimeNs)
    return false;

  if (aN.mtimeNs !== bN.mtimeNs)
    return false;

  if (aN.ctimeNs !== bN.ctimeNs)
    return false;

  if (aN.birthtimeNs !== bN.birthtimeNs)
    return false;

  return true;
}
