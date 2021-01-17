import fs, {Stats}         from 'fs';

import {FakeFS}            from '../FakeFS';
import {Path, convertPath} from '../path';

// 1980-01-01, like Fedora
const defaultTime = new Date(315532800 * 1000);

export type CopyOptions = {
  stableTime: boolean,
  stableSort: boolean,
  overwrite: boolean,
};

export type Operations =
  Array<() => Promise<void>>;

export type LUTimes<P extends Path> =
  Array<[P, Date | number, Date | number]>;

export async function copyPromise<P1 extends Path, P2 extends Path>(destinationFs: FakeFS<P1>, destination: P1, sourceFs: FakeFS<P2>, source: P2, opts: CopyOptions) {
  const normalizedDestination = destinationFs.pathUtils.normalize(destination);
  const normalizedSource = sourceFs.pathUtils.normalize(source);

  const prelayout: Operations = [];
  const postlayout: Operations = [];

  await destinationFs.mkdirPromise(destinationFs.pathUtils.dirname(destination), {recursive: true});

  const updateTime = typeof destinationFs.lutimesPromise === `function`
    ? destinationFs.lutimesPromise.bind(destinationFs)
    : destinationFs.utimesPromise.bind(destinationFs);

  await copyImpl(prelayout, postlayout, updateTime, destinationFs, normalizedDestination, sourceFs, normalizedSource, opts);

  for (const operation of prelayout)
    await operation();

  await Promise.all(postlayout.map(operation => {
    return operation();
  }));
}

async function copyImpl<P1 extends Path, P2 extends Path>(prelayout: Operations, postlayout: Operations, updateTime: typeof FakeFS.prototype.utimesPromise, destinationFs: FakeFS<P1>, destination: P1, sourceFs: FakeFS<P2>, source: P2, opts: CopyOptions) {
  const destinationStat = await maybeLStat(destinationFs, destination);
  const sourceStat = await sourceFs.lstatPromise(source);

  const referenceTime = opts.stableTime
    ? {mtime: defaultTime, atime: defaultTime} as const
    : sourceStat;

  let updated: boolean;
  switch (true) {
    case sourceStat.isDirectory(): {
      updated = await copyFolder(prelayout, postlayout, updateTime, destinationFs, destination, destinationStat, sourceFs, source, sourceStat, opts);
    } break;

    case sourceStat.isFile(): {
      updated = await copyFile(prelayout, postlayout, updateTime, destinationFs, destination, destinationStat, sourceFs, source, sourceStat, opts);
    } break;

    case sourceStat.isSymbolicLink(): {
      updated = await copySymlink(prelayout, postlayout, updateTime, destinationFs, destination, destinationStat, sourceFs, source, sourceStat, opts);
    } break;

    default: {
      throw new Error(`Unsupported file type (${sourceStat.mode})`);
    } break;
  }

  if (updated || destinationStat?.mtime?.getTime() !== referenceTime.mtime.getTime() || destinationStat?.atime?.getTime() !== referenceTime.atime.getTime()) {
    postlayout.push(() => updateTime(destination, referenceTime.atime, referenceTime.mtime));
    updated = true;
  }

  if (destinationStat === null || (destinationStat.mode & 0o777) !== (sourceStat.mode & 0o777)) {
    postlayout.push(() => destinationFs.chmodPromise(destination, sourceStat.mode & 0o777));
    updated = true;
  }

  return updated;
}

async function maybeLStat<P extends Path>(baseFs: FakeFS<P>, p: P) {
  try {
    return await baseFs.lstatPromise(p);
  } catch (e) {
    return null;
  }
}

async function copyFolder<P1 extends Path, P2 extends Path>(prelayout: Operations, postlayout: Operations, updateTime: typeof FakeFS.prototype.utimesPromise, destinationFs: FakeFS<P1>, destination: P1, destinationStat: Stats | null, sourceFs: FakeFS<P2>, source: P2, sourceStat: Stats, opts: CopyOptions) {
  if (destinationStat !== null && !destinationStat.isDirectory()) {
    if (opts.overwrite) {
      prelayout.push(async () => destinationFs.removePromise(destination));
      destinationStat = null;
    } else {
      return false;
    }
  }

  let updated = false;

  if (destinationStat === null) {
    prelayout.push(async () => destinationFs.mkdirPromise(destination, {mode: sourceStat.mode}));
    updated = true;
  }

  const entries = await sourceFs.readdirPromise(source);

  if (opts.stableSort) {
    for (const entry of entries.sort()) {
      if (await copyImpl(prelayout, postlayout, updateTime, destinationFs, destinationFs.pathUtils.join(destination, entry), sourceFs, sourceFs.pathUtils.join(source, entry), opts)) {
        updated = true;
      }
    }
  } else {
    const entriesUpdateStatus = await Promise.all(entries.map(async entry => {
      await copyImpl(prelayout, postlayout, updateTime, destinationFs, destinationFs.pathUtils.join(destination, entry), sourceFs, sourceFs.pathUtils.join(source, entry), opts);
    }));

    if (entriesUpdateStatus.some(status => status)) {
      updated = true;
    }
  }

  return updated;
}

async function copyFile<P1 extends Path, P2 extends Path>(prelayout: Operations, postlayout: Operations, updateTime: typeof FakeFS.prototype.utimesPromise, destinationFs: FakeFS<P1>, destination: P1, destinationStat: Stats | null, sourceFs: FakeFS<P2>, source: P2, sourceStat: Stats, opts: CopyOptions) {
  if (destinationStat !== null) {
    if (opts.overwrite) {
      prelayout.push(async () => destinationFs.removePromise(destination));
      destinationStat = null;
    } else {
      return false;
    }
  }

  const op = destinationFs as any === sourceFs as any
    ? async () => destinationFs.copyFilePromise(source as any as P1, destination, fs.constants.COPYFILE_FICLONE)
    : async () => destinationFs.writeFilePromise(destination, await sourceFs.readFilePromise(source));

  prelayout.push(async () => op());
  return true;
}

async function copySymlink<P1 extends Path, P2 extends Path>(prelayout: Operations, postlayout: Operations, updateTime: typeof FakeFS.prototype.utimesPromise, destinationFs: FakeFS<P1>, destination: P1, destinationStat: Stats | null, sourceFs: FakeFS<P2>, source: P2, sourceStat: Stats, opts: CopyOptions) {
  if (destinationStat !== null) {
    if (opts.overwrite) {
      prelayout.push(async () => destinationFs.removePromise(destination));
      destinationStat = null;
    } else {
      return false;
    }
  }

  prelayout.push(async () => {
    await destinationFs.symlinkPromise(convertPath(destinationFs.pathUtils, await sourceFs.readlinkPromise(source)), destination);
  });

  return true;
}
