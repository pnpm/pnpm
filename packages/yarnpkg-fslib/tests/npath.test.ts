import {npath} from '../sources/path';

describe(`Portable paths`, () => {
  for (const platform of [`darwin`, `win32`]) {
    let realPlatform: string;

    describe(`Platform ${platform}`, () => {
      beforeAll(() => {
        realPlatform = process.platform;
        Object.defineProperty(process, `platform`, {
          configurable: true,
          value: platform,
        });
      });

      afterAll(() => {
        Object.defineProperty(process, `platform`, {
          configurable: true,
          value: realPlatform,
        });
      });

      describe(`toPortablePath`, () => {
        if (platform !== `win32`) {
          it(`shouldn't change paths on non-Windows platform`, () => {
            const inputPath = `C:\\Users\\user\\proj`;
            const outputPath = inputPath;
            expect(npath.toPortablePath(inputPath)).toEqual(outputPath);
          });
        } else {
          it(`shouldn't change absolute posix paths when producing portable path`, () => {
            const inputPath = `/home/user/proj`;
            const outputPath = inputPath;
            expect(npath.toPortablePath(inputPath)).toEqual(outputPath);
          });

          it(`shouldn't change absolute paths that are already portable`, () => {
            const inputPath = `/c:/Users/user/proj`;
            const outputPath = `/c:/Users/user/proj`;
            expect(npath.toPortablePath(inputPath)).toEqual(outputPath);
          });

          it(`should normalize the slashes in relative Windows paths`, () => {
            const inputPath = `..\\Users\\user/proj`;
            const outputPath = `../Users/user/proj`;
            expect(npath.toPortablePath(inputPath)).toEqual(outputPath);
          });

          it(`should transform Windows paths into their posix counterparts (uppercase drive)`, () => {
            const inputPath = `C:\\Users\\user\\proj`;
            const outputPath = `/C:/Users/user/proj`;
            expect(npath.toPortablePath(inputPath)).toEqual(outputPath);
          });

          it(`should transform Windows paths into their posix counterparts (lowercase drive)`, () => {
            const inputPath = `c:\\Users\\user\\proj`;
            const outputPath = `/c:/Users/user/proj`;
            expect(npath.toPortablePath(inputPath)).toEqual(outputPath);
          });

          it(`should transform Windows paths into their posix counterparts (forward slashes)`, () => {
            const inputPath = `C:/Users/user/proj`;
            const outputPath = `/C:/Users/user/proj`;
            expect(npath.toPortablePath(inputPath)).toEqual(outputPath);
          });

          it(`should support Windows paths that contain both backslashes and forward slashes`, () => {
            const inputPath = `C:/Users\\user/proj`;
            const outputPath = `/C:/Users/user/proj`;
            expect(npath.toPortablePath(inputPath)).toEqual(outputPath);
          });

          it(`should support drive: Windows paths`, () => {
            const inputPath = `C:`;
            const outputPath = `/C:`;
            expect(npath.toPortablePath(inputPath)).toEqual(outputPath);
          });

          it(`should support UNC Windows paths (\\\\[server]\\[sharename]\\)`, () => {
            const inputPath = `\\\\Server01\\user\\docs\\Letter.txt`;
            const outputPath = `/unc/Server01/user/docs/Letter.txt`;
            expect(npath.toPortablePath(inputPath)).toEqual(outputPath);
          });

          it(`should support Long UNC Windows paths (\\\\?\\[server]\\[sharename]\\)`, () => {
            const inputPath = `\\\\?\\Server01\\user\\docs\\Letter.txt`;
            const outputPath = `/unc/?/Server01/user/docs/Letter.txt`;
            expect(npath.toPortablePath(inputPath)).toEqual(outputPath);
          });

          it(`should support Long UNC Windows paths (\\\\?\\UNC\\[server]\\[sharename]\\)`, () => {
            const inputPath = `\\\\?\\UNC\\Server01\\user\\docs\\Letter.txt`;
            const outputPath = `/unc/?/UNC/Server01/user/docs/Letter.txt`;
            expect(npath.toPortablePath(inputPath)).toEqual(outputPath);
          });

          it(`should support Long UNC Windows paths (\\\\?\\[drive_spec]:\\)`, () => {
            const inputPath = `\\\\?\\C:\\user\\docs\\Letter.txt`;
            const outputPath = `/unc/?/C:/user/docs/Letter.txt`;
            expect(npath.toPortablePath(inputPath)).toEqual(outputPath);
          });

          it(`should support Long UNC Windows paths with dot (\\\\.\\[physical_device]\\)`, () => {
            const inputPath = `\\\\.\\PhysicalDevice\\user\\docs\\Letter.txt`;
            const outputPath = `/unc/.dot/PhysicalDevice/user/docs/Letter.txt`;
            expect(npath.toPortablePath(inputPath)).toEqual(outputPath);
          });
        }
      });

      describe(`fromPortablePath`, () => {
        if (platform !== `win32`) {
          it(`shouldn't change portable paths on non-Windows platforms`, () => {
            const inputPath = `/c:/Users/user/proj`;
            const outputPath = inputPath;
            expect(npath.fromPortablePath(inputPath)).toEqual(outputPath);
          });
        } else {
          it(`shouldn't change absolute posix paths when producing native path`, () => {
            const inputPath = `/home/user/proj`;
            const outputPath = `/home/user/proj`;
            expect(npath.fromPortablePath(inputPath)).toEqual(outputPath);
          });

          it(`shouldn't change relative posix paths when producing native paths`, () => {
            const inputPath = `../Users/user/proj`;
            const outputPath = inputPath;
            expect(npath.fromPortablePath(inputPath)).toEqual(outputPath);
          });

          it(`shouldn't change absolute path when it is already Windows`, () => {
            const inputPath = `c:\\Users\\user\\proj`;
            const outputPath = inputPath;
            expect(npath.fromPortablePath(inputPath)).toEqual(outputPath);
          });

          it(`should transform back Windows paths on Windows platforms (lowercase drive)`, () => {
            const inputPath = `/c:/Users/user/proj`;
            const outputPath = `c:\\Users\\user\\proj`;
            expect(npath.fromPortablePath(inputPath)).toEqual(outputPath);
          });

          it(`should transform back Windows paths on Windows platforms (uppercase drive)`, () => {
            const inputPath = `/C:/Users/user/proj`;
            const outputPath = `C:\\Users\\user\\proj`;
            expect(npath.fromPortablePath(inputPath)).toEqual(outputPath);
          });

          it(`should transform back Windows paths on Windows platforms (mixed path)`, () => {
            const inputPath = `/c:/Users\\user/proj`;
            const outputPath = `c:\\Users\\user\\proj`;
            expect(npath.fromPortablePath(inputPath)).toEqual(outputPath);
          });

          it(`should transform back drive: on Windows platforms`, () => {
            const inputPath = `/C:`;
            const outputPath = `C:`;
            expect(npath.fromPortablePath(inputPath)).toEqual(outputPath);
          });

          it(`should transform back UNC Windows paths (/unc/[server]/[sharename]/)`, () => {
            const inputPath = `/unc/Server01/user/docs/Letter.txt`;
            const outputPath = `\\\\Server01\\user\\docs\\Letter.txt`;
            expect(npath.fromPortablePath(inputPath)).toEqual(outputPath);
          });

          it(`should transform back Long UNC Windows paths (/unc/?/[server]/[sharename]/)`, () => {
            const inputPath = `/unc/?/Server01/user/docs/Letter.txt`;
            const outputPath = `\\\\?\\Server01\\user\\docs\\Letter.txt`;
            expect(npath.fromPortablePath(inputPath)).toEqual(outputPath);
          });

          it(`should transform back Long UNC Windows paths (/unc/?/UNC/[server]/[sharename]/)`, () => {
            const inputPath = `/unc/?/UNC/Server01/user/docs/Letter.txt`;
            const outputPath = `\\\\?\\UNC\\Server01\\user\\docs\\Letter.txt`;
            expect(npath.fromPortablePath(inputPath)).toEqual(outputPath);
          });

          it(`should transform back Long UNC Windows paths (/unc/?/[drive_spec]:/)`, () => {
            const inputPath = `/unc/?/C:/user/docs/Letter.txt`;
            const outputPath = `\\\\?\\C:\\user\\docs\\Letter.txt`;
            expect(npath.fromPortablePath(inputPath)).toEqual(outputPath);
          });

          it(`should transform back Long UNC Windows paths with dot (/unc/.dot/[physical_device]/)`, () => {
            const inputPath = `/unc/.dot/PhysicalDevice/user/docs/Letter.txt`;
            const outputPath = `\\\\.\\PhysicalDevice\\user\\docs\\Letter.txt`;
            expect(npath.fromPortablePath(inputPath)).toEqual(outputPath);
          });
        }
      });
    });
  }
});
