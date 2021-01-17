import {getLibzipSync}          from '@yarnpkg/libzip';

import {ppath, npath, Filename} from '../sources/path';
import {ZipOpenFS}              from '../sources';

import {useFakeTime}            from './utils';

const ZIP_DIR1 = ppath.join(
  npath.toPortablePath(__dirname),
  `fixtures/foo.zip` as Filename
);
const ZIP_DIR2 = ppath.join(
  npath.toPortablePath(__dirname),
  `fixtures/folder.zip/foo.zip` as Filename
);

export const ZIP_FILE1 = ppath.join(ZIP_DIR1, `foo.txt` as Filename);
const ZIP_FILE2 = ppath.join(ZIP_DIR2, `foo.txt` as Filename);

describe(`ZipOpenFS`, () => {
  it(`can read from a zip file`, () => {
    const fs = new ZipOpenFS({libzip: getLibzipSync()});

    expect(fs.readFileSync(ZIP_FILE1, `utf8`)).toEqual(`foo\n`);

    fs.discardAndClose();
  });

  it(`can read from a zip file in a path containing .zip`, () => {
    const fs = new ZipOpenFS({libzip: getLibzipSync()});

    expect(fs.readFileSync(ZIP_FILE2, `utf8`)).toEqual(`foo\n`);

    fs.discardAndClose();
  });

  it(`doesn't close a ZipFS instance with open handles`, () => {
    const fs = new ZipOpenFS({libzip: getLibzipSync(), maxOpenFiles: 1});

    const fileHandle = fs.openSync(ZIP_FILE1, ``);

    expect(fs.readFileSync(ZIP_FILE2, `utf8`)).toEqual(`foo\n`);

    const buff = Buffer.alloc(4);
    fs.readSync(fileHandle, buff, 0, 4, 0);
    fs.closeSync(fileHandle);

    expect(buff.toString(`utf8`)).toEqual(`foo\n`);

    fs.discardAndClose();
  });

  it(`treats createReadStream as an open file handle`, async () => {
    const fs = new ZipOpenFS({libzip: getLibzipSync(), maxOpenFiles: 1});

    const chunks: Array<Buffer> = [];
    await new Promise<void>(resolve => {
      let done = 0;

      fs.createReadStream(ZIP_FILE1)
        .on(`data`, (chunk: Buffer) => {
          chunks.push(chunk);
        })
        .on(`close`, () => {
          if (++done === 2) {
            resolve();
          }
        });

      fs.createReadStream(ZIP_FILE2)
        .on(`data`, (chunk: Buffer) => {
          chunks.push(chunk);
        })
        .on(`close`, () => {
          if (++done === 2) {
            resolve();
          }
        });
    });

    expect(chunks[0].toString(`utf8`)).toMatch(`foo\n`);
    expect(chunks[1].toString(`utf8`)).toMatch(`foo\n`);

    fs.discardAndClose();
  });

  it(`treats createWriteStream as an open file handle`, async () => {
    const fs = new ZipOpenFS({libzip: getLibzipSync(), maxOpenFiles: 1});

    const stream1 = fs.createWriteStream(ZIP_FILE1);
    const stream2 = fs.createWriteStream(ZIP_FILE2);

    await new Promise<void>(resolve => {
      let done = 0;
      stream1.end(`foo`, () => {
        if (++done === 2) {
          resolve();
        }
      });
      stream2.end(`bar`, () => {
        if (++done === 2) {
          resolve();
        }
      });
    });

    fs.discardAndClose();
  });

  it(`closes ZipFS instances once they become stale`, async () => {
    await useFakeTime(async advanceTimeBy => {
      const fs = new ZipOpenFS({libzip: getLibzipSync(), maxAge: 2000});

      await fs.existsPromise(ZIP_FILE1);
      // @ts-expect-error: zipInstances is private
      expect(fs.zipInstances!.size).toEqual(1);

      advanceTimeBy(1000);

      fs.existsSync(ZIP_FILE2);
      // @ts-expect-error: zipInstances is private
      expect(fs.zipInstances!.size).toEqual(2);

      advanceTimeBy(1000);

      // @ts-expect-error: zipInstances is private
      expect(fs.zipInstances!.size).toEqual(1);

      advanceTimeBy(1000);

      // @ts-expect-error: zipInstances is private
      expect(fs.zipInstances!.size).toEqual(0);

      fs.discardAndClose();
    });
  });

  it(`doesn't close zip files while they are in use`, async () => {
    const fs = new ZipOpenFS({libzip: getLibzipSync(), maxOpenFiles: 1});

    await Promise.all([
      fs.readFilePromise(ZIP_FILE1),
      fs.realpathPromise(ZIP_FILE1),
      fs.readFilePromise(ZIP_FILE2),
      fs.realpathPromise(ZIP_FILE2),
    ]);

    fs.discardAndClose();
  });

  it(`doesn't crash when watching a file in a archive that gets closed`, async () => {
    await useFakeTime(advanceTimeBy => {
      const fs = new ZipOpenFS({libzip: getLibzipSync(), maxOpenFiles: 1});

      fs.watchFile(ZIP_FILE1, (current, previous) => {});
      fs.watchFile(ZIP_FILE2, (current, previous) => {});

      advanceTimeBy(100);

      fs.discardAndClose();
    });
  });

  it(`treats Dir instances opened via opendir as open file handles`, () => {
    const fs = new ZipOpenFS({libzip: getLibzipSync(), maxOpenFiles: 1});

    const dir1 = fs.opendirSync(ZIP_DIR1);
    const dir2 = fs.opendirSync(ZIP_DIR2);

    expect(dir1.readSync()!.name).toStrictEqual(`foo.txt`);
    expect(dir2.readSync()!.name).toStrictEqual(`foo.txt`);

    fs.discardAndClose();
  });
});
