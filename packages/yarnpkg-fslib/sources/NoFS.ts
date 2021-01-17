import {FakeFS}              from './FakeFS';
import {PortablePath, ppath} from './path';

const makeError = () => Object.assign(new Error(`ENOSYS: unsupported filesystem access`), {code: `ENOSYS`});

export class NoFS extends FakeFS<PortablePath> {
  static readonly instance = new NoFS();

  constructor() {
    super(ppath);
  }

  getExtractHint(): never {
    throw makeError();
  }

  getRealPath(): never {
    throw makeError();
  }

  resolve(): never {
    throw makeError();
  }

  async openPromise(): Promise<never> {
    throw makeError();
  }

  openSync(): never {
    throw makeError();
  }

  async opendirPromise(): Promise<never> {
    throw makeError();
  }

  opendirSync(): never {
    throw makeError();
  }

  async readPromise(): Promise<never> {
    throw makeError();
  }

  readSync(): never {
    throw makeError();
  }

  async writePromise(): Promise<never> {
    throw makeError();
  }

  writeSync(): never {
    throw makeError();
  }

  async closePromise(): Promise<never> {
    throw makeError();
  }

  closeSync(): never {
    throw makeError();
  }

  createWriteStream(): never {
    throw makeError();
  }

  createReadStream(): never {
    throw makeError();
  }

  async realpathPromise(): Promise<never> {
    throw makeError();
  }

  realpathSync(): never {
    throw makeError();
  }

  async readdirPromise(): Promise<never> {
    throw makeError();
  }

  readdirSync(): never {
    throw makeError();
  }

  async existsPromise(p: PortablePath): Promise<never> {
    throw makeError();
  }

  existsSync(p: PortablePath): never {
    throw makeError();
  }

  async accessPromise(): Promise<never> {
    throw makeError();
  }

  accessSync(): never {
    throw makeError();
  }

  async statPromise(): Promise<never> {
    throw makeError();
  }

  statSync(): never {
    throw makeError();
  }

  async fstatPromise(fd: number): Promise<never> {
    throw makeError();
  }

  fstatSync(fd: number): never {
    throw makeError();
  }

  async lstatPromise(p: PortablePath): Promise<never> {
    throw makeError();
  }

  lstatSync(p: PortablePath): never {
    throw makeError();
  }

  async chmodPromise(): Promise<never> {
    throw makeError();
  }

  chmodSync(): never {
    throw makeError();
  }

  async chownPromise(): Promise<never> {
    throw makeError();
  }

  chownSync(): never {
    throw makeError();
  }

  async mkdirPromise(): Promise<never> {
    throw makeError();
  }

  mkdirSync(): never {
    throw makeError();
  }

  async rmdirPromise(): Promise<never> {
    throw makeError();
  }

  rmdirSync(): never {
    throw makeError();
  }

  async linkPromise(): Promise<never> {
    throw makeError();
  }

  linkSync(): never {
    throw makeError();
  }

  async symlinkPromise(): Promise<never> {
    throw makeError();
  }

  symlinkSync(): never {
    throw makeError();
  }

  async renamePromise(): Promise<never> {
    throw makeError();
  }

  renameSync(): never {
    throw makeError();
  }

  async copyFilePromise(): Promise<never> {
    throw makeError();
  }

  copyFileSync(): never {
    throw makeError();
  }

  async appendFilePromise(): Promise<never> {
    throw makeError();
  }

  appendFileSync(): never {
    throw makeError();
  }

  async writeFilePromise(): Promise<never> {
    throw makeError();
  }

  writeFileSync(): never {
    throw makeError();
  }

  async unlinkPromise(): Promise<never> {
    throw makeError();
  }

  unlinkSync(): never {
    throw makeError();
  }

  async utimesPromise(): Promise<never> {
    throw makeError();
  }

  utimesSync(): never {
    throw makeError();
  }

  async readFilePromise(): Promise<never> {
    throw makeError();
  }

  readFileSync(): never {
    throw makeError();
  }

  async readlinkPromise(): Promise<never> {
    throw makeError();
  }

  readlinkSync(): never {
    throw makeError();
  }

  async truncatePromise(): Promise<never> {
    throw makeError();
  }

  truncateSync(): never {
    throw makeError();
  }

  watch(): never {
    throw makeError();
  }

  watchFile(): never {
    throw makeError();
  }

  unwatchFile(): never {
    throw makeError();
  }
}
