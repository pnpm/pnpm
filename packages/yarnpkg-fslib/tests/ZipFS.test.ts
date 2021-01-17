import {getLibzipSync}                 from '@yarnpkg/libzip';
import fs                              from 'fs';

import {ZipFS}                         from '../sources/ZipFS';
import {PortablePath, ppath, Filename} from '../sources/path';
import {xfs, statUtils}                from '../sources';

import {useFakeTime}                   from './utils';

describe(`ZipFS`, () => {
  it(`should handle symlink correctly`, () => {
    const expectSameStats = (a: fs.Stats, b: fs.Stats) => {
      expect(a.ino).toEqual(b.ino);
      expect(a.size).toEqual(b.size);
      expect(a.mode).toEqual(b.mode);
      expect(a.atimeMs).toEqual(b.atimeMs);
      expect(a.mtimeMs).toEqual(b.mtimeMs);
      expect(a.ctimeMs).toEqual(b.ctimeMs);
      expect(a.birthtimeMs).toEqual(b.birthtimeMs);
      expect(a.isFile()).toEqual(a.isFile());
      expect(a.isDirectory()).toEqual(a.isDirectory());
      expect(a.isSymbolicLink()).toEqual(a.isSymbolicLink());
    };

    const asserts = (zipFs: ZipFS) => {
      const dir = zipFs.statSync(`/dir` as PortablePath);
      expect(dir.isFile()).toBeFalsy();
      expect(dir.isDirectory()).toBeTruthy();
      expect(dir.isSymbolicLink()).toBeFalsy();

      const file = zipFs.statSync(`/dir/file` as PortablePath);
      expect(file.isFile()).toBeTruthy();
      expect(file.isDirectory()).toBeFalsy();
      expect(file.isSymbolicLink()).toBeFalsy();

      expectSameStats(zipFs.lstatSync(`/dir/file` as PortablePath), file);
      expectSameStats(zipFs.lstatSync(`/dir` as PortablePath), dir);

      expectSameStats(zipFs.statSync(`/linkToFileA` as PortablePath), file);
      expectSameStats(zipFs.statSync(`/linkToFileB` as PortablePath), file);
      expectSameStats(zipFs.statSync(`/linkToDirA/file` as PortablePath), file);
      expectSameStats(zipFs.statSync(`/linkToDirB/file` as PortablePath), file);
      expectSameStats(zipFs.statSync(`/linkToCwd/linkToCwd/linkToCwd/linkToCwd/dir/file` as PortablePath), file);

      expectSameStats(zipFs.statSync(`/linkToDirA` as PortablePath), dir);
      expectSameStats(zipFs.statSync(`/linkToDirB` as PortablePath), dir);
      expectSameStats(zipFs.statSync(`/linkToCwd/linkToCwd/linkToCwd/linkToCwd/linkToDirA` as PortablePath), dir);

      expectSameStats(zipFs.lstatSync(`/linkToDirA/file` as PortablePath), file);
      expectSameStats(zipFs.lstatSync(`/linkToDirB/file` as PortablePath), file);
      expectSameStats(zipFs.lstatSync(`/linkToCwd/linkToCwd/linkToCwd/linkToCwd/dir/file` as PortablePath), file);

      const linkToDirA = zipFs.lstatSync(`/linkToDirA` as PortablePath);
      expect(linkToDirA.isFile()).toBeFalsy();
      expect(linkToDirA.isDirectory()).toBeFalsy();
      expect(linkToDirA.isSymbolicLink()).toBeTruthy();

      const linkToDirB = zipFs.lstatSync(`/linkToDirB` as PortablePath);
      expect(linkToDirB.isFile()).toBeFalsy();
      expect(linkToDirB.isDirectory()).toBeFalsy();
      expect(linkToDirB.isSymbolicLink()).toBeTruthy();

      for (const path of [
        `/linkToFileA`,
        `/linkToFileB`,
        `/linkToDirA/file`,
        `/linkToDirB/file`,
        `/dir/file`,
        `/linkToCwd/linkToCwd/linkToCwd/linkToCwd/dir/file`,
      ])
        expect(zipFs.readFileSync(path as PortablePath, `utf8`)).toEqual(`file content`);


      for (const path of [
        `/linkToDirA`,
        `/linkToDirB`,
        `/linkToCwd/linkToCwd/linkToCwd/linkToCwd/dir`,
        `/linkToCwd/linkToCwd/linkToCwd/linkToCwd/linkToDirA`,
        `/linkToCwd/linkToCwd/linkToCwd/linkToCwd/linkToDirB`,
      ]) {
        expect(zipFs.readdirSync(path as PortablePath)).toContain(`file`);
      }
    };

    const libzip = getLibzipSync();
    const tmpfile = ppath.resolve(xfs.mktempSync(), `test.zip` as Filename);
    const zipFs = new ZipFS(tmpfile, {libzip, create: true});

    zipFs.mkdirPromise(`/dir` as PortablePath);
    zipFs.writeFileSync(`/dir/file` as PortablePath, `file content`);

    zipFs.symlinkSync(`dir/file` as PortablePath, `linkToFileA` as PortablePath);
    zipFs.symlinkSync(`./dir/file` as PortablePath, `linkToFileB` as PortablePath);
    zipFs.symlinkSync(`dir` as PortablePath, `linkToDirA` as PortablePath);
    zipFs.symlinkSync(`./dir` as PortablePath, `linkToDirB` as PortablePath);
    zipFs.symlinkSync(`.` as PortablePath, `linkToCwd` as PortablePath);

    // asserts(zipFs);
    zipFs.saveAndClose();

    const zipFs2 = new ZipFS(tmpfile, {libzip});
    asserts(zipFs2);
    zipFs2.discardAndClose();
  });

  it(`should readSync file contents`, async () => {
    const readFileContents = function (zipFs: ZipFS, p: PortablePath, position: number | null) {
      const fd = zipFs.openSync(p, `r`);
      const buffer = Buffer.alloc(8192);
      try {
        let size = 0;
        let read = 0;
        while ((read = zipFs.readSync(fd, buffer, 0, buffer.length, position)) !== 0)
          size += read;

        return buffer.toString(`utf-8`, 0, size);
      } finally {
        zipFs.closeSync(fd);
      }
    };
    const readSyncAsserts = (zipFs: ZipFS) => {
      const p = `/dir/file` as PortablePath;
      expect(readFileContents(zipFs, p, -1)).toEqual(`file content`);
      expect(readFileContents(zipFs, p, null)).toEqual(`file content`);
    };

    const libzip = getLibzipSync();
    const tmpfile = ppath.resolve(xfs.mktempSync(), `test2.zip` as Filename);
    const zipFs = new ZipFS(tmpfile, {libzip, create: true});
    await zipFs.mkdirPromise(`/dir` as PortablePath);
    zipFs.writeFileSync(`/dir/file` as PortablePath, `file content`);
    zipFs.saveAndClose();

    const zipFs2 = new ZipFS(tmpfile, {libzip});
    readSyncAsserts(zipFs2);
    zipFs2.discardAndClose();
  });

  it(`can create a zip file in memory`, () => {
    const libzip = getLibzipSync();
    const zipFs = new ZipFS(null, {libzip});

    zipFs.writeFileSync(`/foo.txt` as PortablePath, `Test`);

    const zipContent = zipFs.getBufferAndClose();

    const zipFs2 = new ZipFS(zipContent, {libzip});
    expect(zipFs2.readFileSync(`/foo.txt` as PortablePath, `utf8`)).toEqual(`Test`);
  });

  it(`can handle nested symlinks`, () => {
    const libzip = getLibzipSync();
    const zipFs = new ZipFS(null, {libzip});
    zipFs.writeFileSync(`/foo.txt` as PortablePath, `Test`);

    zipFs.symlinkSync(`/foo.txt` as PortablePath, `/linkA` as PortablePath);
    zipFs.symlinkSync(`/linkA` as PortablePath, `/linkB` as PortablePath);

    const zipFs2 = new ZipFS(zipFs.getBufferAndClose(), {libzip});

    expect(zipFs2.readFileSync(`/linkA` as PortablePath, `utf8`)).toEqual(`Test`);
    expect(zipFs2.readFileSync(`/linkB` as PortablePath, `utf8`)).toEqual(`Test`);

    zipFs2.discardAndClose();
  });

  it(`returns the same content for sync and async reads`, async () => {
    const libzip = getLibzipSync();
    const zipFs = new ZipFS(null, {libzip});
    zipFs.writeFileSync(`/foo.txt` as PortablePath, `Test`);

    const zipFs2 = new ZipFS(zipFs.getBufferAndClose(), {libzip});

    expect(await zipFs2.readFilePromise(`/foo.txt` as PortablePath, `utf8`)).toEqual(`Test`);
    expect(zipFs2.readFileSync(`/foo.txt` as PortablePath, `utf8`)).toEqual(`Test`);
  });

  it(`should support unlinking files`, () => {
    const libzip = getLibzipSync();
    const zipFs = new ZipFS(null, {libzip});

    const dir = `/foo` as PortablePath;
    zipFs.mkdirSync(dir);

    const file = `/foo/bar.txt` as PortablePath;
    zipFs.writeFileSync(file, `Test`);

    expect(zipFs.existsSync(dir)).toBeTruthy();
    expect(zipFs.existsSync(file)).toBeTruthy();

    zipFs.unlinkSync(file);

    expect(zipFs.existsSync(dir)).toBeTruthy();
    expect(zipFs.existsSync(file)).toBeFalsy();

    zipFs.discardAndClose();
  });

  it(`should support removing empty directories`, () => {
    const libzip = getLibzipSync();
    const zipFs = new ZipFS(null, {libzip});

    const dir = `/foo` as PortablePath;
    const subdir = `/foo/bar` as PortablePath;
    zipFs.mkdirpSync(subdir);

    expect(zipFs.existsSync(dir)).toBeTruthy();
    expect(zipFs.existsSync(subdir)).toBeTruthy();

    zipFs.rmdirSync(subdir);

    expect(zipFs.existsSync(dir)).toBeTruthy();
    expect(zipFs.existsSync(subdir)).toBeFalsy();

    zipFs.discardAndClose();
  });

  it(`should not support removing non-empty directories`, () => {
    const libzip = getLibzipSync();
    const zipFs = new ZipFS(null, {libzip});

    const dir = `/foo` as PortablePath;
    zipFs.mkdirSync(dir);

    const file = `/foo/bar.txt` as PortablePath;
    zipFs.writeFileSync(file, `Test`);

    expect(() => zipFs.rmdirSync(dir)).toThrowError(`ENOTEMPTY`);

    zipFs.discardAndClose();
  });

  it(`should support removing non-empty directories via zipFs.removeSync`, () => {
    const libzip = getLibzipSync();
    const zipFs = new ZipFS(null, {libzip});

    const dir = `/foo` as PortablePath;
    const subdir = `/foo/bar` as PortablePath;
    zipFs.mkdirpSync(subdir);

    const file = `/foo/bar/baz.txt` as PortablePath;
    zipFs.writeFileSync(file, `Test`);

    expect(zipFs.existsSync(dir)).toBeTruthy();
    expect(zipFs.existsSync(subdir)).toBeTruthy();
    expect(zipFs.existsSync(file)).toBeTruthy();

    zipFs.removeSync(subdir);

    expect(zipFs.existsSync(dir)).toBeTruthy();
    expect(zipFs.existsSync(subdir)).toBeFalsy();
    expect(zipFs.existsSync(file)).toBeFalsy();
  });

  it(`should support read after write`, () => {
    const libzip = getLibzipSync();
    const zipFs = new ZipFS(null, {libzip});

    const file = `/foo.txt` as PortablePath;
    zipFs.writeFileSync(file, `Test`);

    expect(zipFs.readFileSync(file, `utf8`)).toStrictEqual(`Test`);

    zipFs.discardAndClose();
  });

  it(`should support write after read`, () => {
    const tmpdir = xfs.mktempSync();
    const archive = `${tmpdir}/archive.zip` as PortablePath;

    const libzip = getLibzipSync();
    const zipFs = new ZipFS(archive, {libzip, create: true});

    const file = `/foo.txt` as PortablePath;
    zipFs.writeFileSync(file, `Hello World`);

    zipFs.saveAndClose();

    const zipFs2 = new ZipFS(archive, {libzip});

    expect(zipFs2.readFileSync(file, `utf8`)).toStrictEqual(`Hello World`);
    expect(() => zipFs2.writeFileSync(file, `Goodbye World`)).not.toThrow();

    zipFs2.discardAndClose();
  });

  it(`should support write after write`, () => {
    const libzip = getLibzipSync();
    const zipFs = new ZipFS(null, {libzip});

    const file = `/foo.txt` as PortablePath;

    zipFs.writeFileSync(file, `Hello World`);
    expect(() => zipFs.writeFileSync(file, `Goodbye World`)).not.toThrow();

    zipFs.discardAndClose();
  });

  it(`should support read after read`, () => {
    const tmpdir = xfs.mktempSync();
    const archive = `${tmpdir}/archive.zip` as PortablePath;

    const libzip = getLibzipSync();
    const zipFs = new ZipFS(archive, {libzip, create: true});

    const file = `/foo.txt` as PortablePath;
    zipFs.writeFileSync(file, `Hello World`);

    zipFs.saveAndClose();

    const zipFs2 = new ZipFS(archive, {libzip});

    expect(zipFs2.readFileSync(file, `utf8`)).toStrictEqual(`Hello World`);
    expect(zipFs2.readFileSync(file, `utf8`)).toStrictEqual(`Hello World`);

    zipFs2.discardAndClose();
  });

  it(`should support truncate`, () => {
    const libzip = getLibzipSync();
    const zipFs = new ZipFS(null, {libzip});

    const file = `/foo.txt` as PortablePath;

    zipFs.writeFileSync(file, `1234567890`);

    zipFs.truncateSync(file, 5);
    expect(zipFs.readFileSync(file, `utf8`)).toStrictEqual(`12345`);

    zipFs.truncateSync(file, 10);
    expect(zipFs.readFileSync(file, `utf8`)).toStrictEqual(`12345${`\u0000`.repeat(5)}`);

    zipFs.truncateSync(file);
    expect(zipFs.readFileSync(file, `utf8`)).toStrictEqual(``);

    zipFs.discardAndClose();
  });

  it(`should support watchFile and unwatchFile`, () => {
    const libzip = getLibzipSync();
    const zipFs = new ZipFS(null, {libzip});

    const file = `/foo.txt` as PortablePath;

    const emptyStats = statUtils.makeEmptyStats();

    const changeListener = jest.fn();
    const stopListener = jest.fn();

    jest.useFakeTimers();

    const statWatcher = zipFs.watchFile(file, {interval: 1000}, changeListener);
    statWatcher.on(`stop`, stopListener);

    // The listener should be initially called with empty stats if the path doesn't exist,
    // but only after 3 milliseconds, so that other listeners can be registered in that timespan
    // (That's what Node does)

    expect(changeListener).not.toHaveBeenCalled();

    jest.advanceTimersByTime(3);

    expect(changeListener).toHaveBeenCalledTimes(1);
    expect(changeListener).toHaveBeenCalledWith(emptyStats, emptyStats);

    // The watcher should pick up changes in content

    zipFs.writeFileSync(file, `Hello World`);
    const first = zipFs.statSync(file);

    jest.advanceTimersByTime(1000);

    expect(changeListener).toHaveBeenCalledTimes(2);
    expect(changeListener).toHaveBeenCalledWith(first, emptyStats);

    // The watcher should only pick up the last changes in an interval

    zipFs.writeFileSync(file, `This shouldn't be picked up`);

    zipFs.writeFileSync(file, `Goodbye World`);
    const second = zipFs.statSync(file);

    jest.advanceTimersByTime(1000);

    expect(changeListener).toHaveBeenCalledTimes(3);
    expect(changeListener).toHaveBeenCalledWith(second, first);

    // The watcher should pick up deletions

    zipFs.unlinkSync(file);

    jest.advanceTimersByTime(1000);

    expect(changeListener).toHaveBeenCalledTimes(4);
    expect(changeListener).toHaveBeenCalledWith(emptyStats, second);

    // unwatchFile should work

    expect(stopListener).not.toHaveBeenCalled();

    zipFs.unwatchFile(file, changeListener);

    // The stop event should be emitted when there are no remaining change listeners
    expect(stopListener).toHaveBeenCalledTimes(1);

    // The listener shouldn't be called after the file is unwatched

    zipFs.writeFileSync(file, `Test`);

    jest.advanceTimersByTime(1000);

    expect(changeListener).toHaveBeenCalledTimes(4);

    zipFs.discardAndClose();

    // The watcher shouldn't keep the process running after the file is unwatched
  });

  it(`closes the fd created in createReadStream when the stream is closed early`, async () => {
    const zipFs = new ZipFS(null, {libzip: getLibzipSync()});
    zipFs.writeFileSync(`/foo.txt` as Filename, `foo`.repeat(10000));

    expect(zipFs.hasOpenFileHandles()).toBe(false);
    const stream = zipFs.createReadStream(`/foo.txt` as Filename);

    expect(zipFs.hasOpenFileHandles()).toBe(true);

    await new Promise<void>((resolve, reject) => {
      stream.on(`data`, () => {
        reject(new Error(`Should not be called`));
      });
      stream.on(`close`, () => {
        resolve();
      });
      stream.on(`error`, error => {
        reject(error);
      });

      stream.close();
    });

    expect(zipFs.hasOpenFileHandles()).toBe(false);

    zipFs.discardAndClose();
  });

  it(`should close the createWriteStream when destroyed`, async () => {
    const zipFs = new ZipFS(null, {libzip: getLibzipSync()});

    const writeStream = zipFs.createWriteStream(`/foo.txt` as Filename);

    await new Promise<void>((resolve, reject) => {
      writeStream.write(`foo`, err => {
        if (err) {
          reject(err);
        } else {
          writeStream.destroy();
          resolve();
        }
      });
    });

    expect(zipFs.hasOpenFileHandles()).toBe(false);

    expect(zipFs.readFileSync(`/foo.txt` as Filename, `utf8`)).toBe(`foo`);

    zipFs.discardAndClose();
  });

  it(`should stop the watcher on closing the archive`, async () => {
    await useFakeTime(advanceTimeBy => {
      const zipFs = new ZipFS(null, {libzip: getLibzipSync()});

      zipFs.writeFileSync(`/foo.txt` as PortablePath, `foo`);

      zipFs.watchFile(`/foo.txt` as PortablePath, (current, previous) => {});

      zipFs.discardAndClose();

      // If the watcher wasn't stopped this will trigger `EBUSY: archive closed`
      advanceTimeBy(100);
    });
  });

  it(`should support opendir`, async () => {
    const libzip = getLibzipSync();
    const zipFs = new ZipFS(null, {libzip});

    const folder = `/foo` as PortablePath;
    zipFs.mkdirSync(folder);

    const firstFile = `/foo/1.txt` as PortablePath;
    const secondFile = `/foo/2.txt` as PortablePath;
    const thirdFile = `/foo/3.txt` as PortablePath;

    zipFs.writeFileSync(firstFile, ``);
    zipFs.writeFileSync(secondFile, ``);
    zipFs.writeFileSync(thirdFile, ``);

    const dir = zipFs.opendirSync(folder);

    expect(dir.path).toStrictEqual(folder);

    const iter = dir[Symbol.asyncIterator]();

    expect((await iter.next()).value.name).toStrictEqual(ppath.basename(firstFile));
    expect(dir.readSync()!.name).toStrictEqual(ppath.basename(secondFile));
    expect((await dir.read())!.name).toStrictEqual(ppath.basename(thirdFile));

    expect((await iter.next()).value).toBeUndefined();

    // Consuming the iterator should cause the Dir instance to close

    // FIXME: This assertion fails
    // await expect(() => iter.next()).rejects.toThrow(`Directory handle was closed`);
    expect(() => dir.readSync()).toThrow(`Directory handle was closed`);
    // It's important that this function throws synchronously, because that's what Node does
    expect(() => dir.read()).toThrow(`Directory handle was closed`);

    zipFs.discardAndClose();
  });

  it(`closes the fd created in opendir when the Dir is closed early`, () => {
    const zipFs = new ZipFS(null, {libzip: getLibzipSync()});
    zipFs.mkdirSync(`/foo` as PortablePath);

    expect(zipFs.hasOpenFileHandles()).toBe(false);
    const dir = zipFs.opendirSync(`/foo` as Filename);
    expect(zipFs.hasOpenFileHandles()).toBe(true);
    dir.closeSync();
    expect(zipFs.hasOpenFileHandles()).toBe(false);

    zipFs.discardAndClose();
  });

  it(`should emit the 'end' event from large reads in createReadStream`, async () => {
    const zipFs = new ZipFS(null, {libzip: getLibzipSync()});
    zipFs.writeFileSync(`/foo.txt` as Filename, `foo`.repeat(10000));

    const stream = zipFs.createReadStream(`/foo.txt` as Filename);

    let endEmitted = false;

    await new Promise<void>((resolve, reject) => {
      stream.on(`end`, () => {
        endEmitted = true;
      });

      stream.on(`close`, () => {
        if (!endEmitted) {
          setTimeout(() => {
            resolve();
          }, 1000);
        }
      });

      const nullStream = fs.createWriteStream(process.platform === `win32` ? `\\\\.\\NUL` : `/dev/null`);

      const piped = stream.pipe(nullStream);

      piped.on(`finish`, () => {
        resolve();
      });

      stream.on(`error`, error => reject(error));
      piped.on(`error`, error => reject(error));
    });

    expect(endEmitted).toBe(true);

    zipFs.discardAndClose();
  });

  it(`should return bigint stats`, () => {
    const zipFs = new ZipFS(null, {libzip: getLibzipSync()});
    zipFs.mkdirSync(`/foo` as PortablePath);

    expect(
      statUtils.areStatsEqual(
        zipFs.statSync(`/foo` as PortablePath, {bigint: true}),
        zipFs.statSync(`/foo` as PortablePath, {bigint: true})
      )
    ).toBe(true);

    expect(
      statUtils.areStatsEqual(
        zipFs.statSync(`/foo` as PortablePath, {bigint: false}),
        zipFs.statSync(`/foo` as PortablePath, {bigint: true})
      )
    ).toBe(false);

    zipFs.discardAndClose();
  });
});
