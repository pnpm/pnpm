import {getLibzipPromise}     from '@yarnpkg/libzip';
import fs                     from 'fs';
import {pathToFileURL}        from 'url';
import {promisify}            from 'util';

import {NodeFS}               from '../sources/NodeFS';
import {PosixFS}              from '../sources/PosixFS';
import {extendFs}             from '../sources/patchFs';
import {Filename, npath}      from '../sources/path';
import {xfs}                  from '../sources/xfs';
import {statUtils, ZipOpenFS} from '../sources';

import {ZIP_FILE1}            from "./ZipOpenFS.test";

describe(`patchedFs`, () => {
  it(`in case of no error, give null: fs.stat`, done => {
    const file = npath.join(__dirname, `patchedFs.test.ts` as Filename);

    const patchedFs = extendFs(fs, new PosixFS(new NodeFS()));

    patchedFs.stat(file, err => {
      expect(err).toEqual(null);
      done();
    });
  });

  it(`in case of no error, give null: fs.read`, done => {
    const file = npath.join(__dirname, `patchedFs.test.ts` as Filename);

    const patchedFs = extendFs(fs, new PosixFS(new NodeFS()));

    const id = patchedFs.openSync(file, `r`);

    patchedFs.read(id, Buffer.alloc(1), 0, 1, 0, err => {
      patchedFs.closeSync(id);
      expect(err).toEqual(null);
      done();
    });
  });

  it(`in case of the parameter of fs.exists is not a string, give false`, done => {
    const patchedFs = extendFs(fs, new PosixFS(new NodeFS()));

    patchedFs.exists(undefined as any, exists => {
      expect(exists).toBe(false);
      done();
    });
  });

  it(`matches the util.promisify return shape of node: fs.read`, async () => {
    const patchedFs = extendFs(fs, new PosixFS(new NodeFS()));
    const patchedFsReadAsync = promisify(patchedFs.read);

    const file = npath.join(__dirname, `patchedFs.test.ts` as Filename);

    const fd = fs.openSync(file, `r`);

    const bufferFs = Buffer.alloc(16);

    const result = await patchedFsReadAsync(fd, bufferFs, 0, 16, 0);

    expect(typeof result.bytesRead).toBe(`number`);
    expect(Buffer.isBuffer(result.buffer)).toBeTruthy();
  });

  it(`should support URL instances`, () => {
    const patchedFs = extendFs(fs, new PosixFS(new NodeFS()));

    const tmpdir = npath.fromPortablePath(xfs.mktempSync());
    const tmpdirUrl = pathToFileURL(tmpdir);

    const file = `${tmpdir}/file.txt`;
    const fileUrl = pathToFileURL(file);

    patchedFs.writeFileSync(fileUrl, `Hello World`);

    expect(patchedFs.readdirSync(tmpdirUrl)).toStrictEqual(patchedFs.readdirSync(tmpdir));

    expect(patchedFs.readFileSync(fileUrl, {encoding: `utf8`})).toStrictEqual(patchedFs.readFileSync(file, {encoding: `utf8`}));
    expect(patchedFs.statSync(fileUrl)).toStrictEqual(patchedFs.statSync(file));

    const copyUrl = pathToFileURL(`${tmpdir}/copy.txt`);
    const renamedUrl = pathToFileURL(`${tmpdir}/renamed.txt`);

    patchedFs.copyFileSync(fileUrl, copyUrl);
    patchedFs.renameSync(copyUrl, renamedUrl);
    patchedFs.unlinkSync(renamedUrl);

    expect(patchedFs.existsSync(renamedUrl)).toStrictEqual(false);
  });

  it(`should support fstat`, async () => {
    const patchedFs = extendFs(fs, new PosixFS(new ZipOpenFS({libzip: await getLibzipPromise(), baseFs: new NodeFS()})));

    const fd = patchedFs.openSync(__filename, `r`);
    try {
      const stat = patchedFs.statSync(__filename);
      const fdStat = patchedFs.fstatSync(fd);

      expect(statUtils.areStatsEqual(stat, fdStat)).toEqual(true);
    } finally {
      patchedFs.closeSync(fd);
    }

    const zipFd = patchedFs.openSync(ZIP_FILE1, `r`);
    try {
      const stat = await new Promise<fs.Stats>((resolve, reject) => {
        patchedFs.stat(ZIP_FILE1, (err, stats) => {
          err ? reject(err) : resolve(stats);
        });
      });

      const fdStat = await new Promise<fs.Stats>((resolve, reject) => {
        patchedFs.fstat(zipFd, (err, stats) => {
          err ? reject(err) : resolve(stats);
        });
      });

      expect(statUtils.areStatsEqual(stat, fdStat)).toEqual(true);
    } finally {
      patchedFs.closeSync(fd);
    }
  });
});
