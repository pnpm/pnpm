import {JailFS}                             from '../sources/JailFS';
import {xfs, ppath, PortablePath, Filename} from '../sources';

describe(`JailFS`, () => {
  it(`should not throw an error when the accessed path is inside the target folder (relative)`, async () => {
    const tmpdir = await xfs.mktempPromise();
    const jailedFolder = ppath.join(tmpdir, `jailed` as PortablePath);

    await xfs.mkdirPromise(jailedFolder);

    const jailFs = new JailFS(jailedFolder);
    await jailFs.writeFilePromise(`text.txt` as Filename, `Hello World`);
  });

  it(`should not throw an error when the accessed path is inside the target folder (absolute)`, async () => {
    const tmpdir = await xfs.mktempPromise();
    const jailedFolder = ppath.join(tmpdir, `jailed` as PortablePath);

    await xfs.mkdirPromise(jailedFolder);

    const jailFs = new JailFS(jailedFolder);
    jailFs.writeFilePromise(ppath.join(PortablePath.root, `text.txt` as Filename), `Hello World`);
  });

  it(`should throw an error when the accessed path is not inside the target folder`, async () => {
    const tmpdir = await xfs.mktempPromise();
    const proxyFolder = ppath.join(tmpdir, `proxy` as PortablePath);
    const jailedFolder = ppath.join(proxyFolder, `jailed` as PortablePath);

    await xfs.mkdirpPromise(jailedFolder);

    const jailFs = new JailFS(jailedFolder);
    await expect(jailFs.writeFilePromise(`../text.txt` as Filename, `Hello World`)).rejects.toThrow(`Resolving this path (../text.txt) would escape the jail`);
  });
});
