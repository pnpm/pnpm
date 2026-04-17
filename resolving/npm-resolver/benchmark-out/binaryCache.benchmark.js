/**
 * Benchmark comparing binary cache (msgpackr + SQLite) vs JSON.parse/stringify
 *
 * This is a benchmark file, not a test. Run directly with:
 *   node --experimental-vm-modules dist/test/binaryCache.benchmark.js
 *
 * Or compile and run:
 *   pnpm --filter @pnpm/resolving.npm-resolver run compile
 *   node resolving/npm-resolver/lib/test/binaryCache.benchmark.js
 */
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { Packr } from 'msgpackr';
import { temporaryDirectory } from 'tempy';
import { RegistryMetadataCache, closeAllRegistryMetadataCaches, } from '@pnpm/resolving.registry-metadata-cache';
const __dirname = path.dirname(fileURLToPath(import.meta.url));
const packr = new Packr({
    useRecords: true,
    moreTypes: true,
});
// Load a real fixture to use as template, or create synthetic data if not available
function loadFixture(name) {
    // Try source fixtures dir first since this file is compiled to lib/test/
    const fixturePath = path.join(__dirname, '..', '..', 'test', 'fixtures', `${name}.json`);
    try {
        return JSON.parse(fs.readFileSync(fixturePath, 'utf-8'));
    }
    catch {
        // Fallback: create synthetic fixture data
        return {
            name: 'is-positive',
            'dist-tags': { latest: '3.1.0' },
            versions: {
                '1.0.0': { name: 'is-positive', version: '1.0.0' },
                '2.0.0': { name: 'is-positive', version: '2.0.0' },
                '3.0.0': { name: 'is-positive', version: '3.0.0' },
                '3.1.0': { name: 'is-positive', version: '3.1.0' },
            },
        };
    }
}
// Create a larger package metadata object for benchmarking
function createLargePackageMeta(versionCount) {
    const versions = {};
    for (let i = 0; i < versionCount; i++) {
        const version = `${Math.floor(i / 100)}.${Math.floor((i % 100) / 10)}.${i % 10}`;
        versions[version] = {
            name: 'benchmark-pkg',
            version,
            dependencies: {
                'dep-a': '^1.0.0',
                'dep-b': '^2.0.0',
                'dep-c': '^3.0.0',
            },
            devDependencies: {
                'dev-dep-a': '^1.0.0',
                'dev-dep-b': '^2.0.0',
            },
            dist: {
                integrity: `sha512-${Buffer.from(`integrity-${i}`).toString('base64')}`,
                tarball: `https://registry.npmjs.org/benchmark-pkg/-/benchmark-pkg-${version}.tgz`,
            },
            engines: {
                node: '>=14',
            },
        };
    }
    return {
        name: 'benchmark-pkg',
        'dist-tags': { latest: `${Math.floor(versionCount / 100) - 1}.9.9` },
        versions,
        etag: '"benchmark-etag"',
        modified: new Date().toUTCString(),
    };
}
function benchmark(name, fn, iterations) {
    // Warmup
    for (let i = 0; i < Math.min(iterations / 10, 100); i++) {
        fn();
    }
    const start = performance.now();
    for (let i = 0; i < iterations; i++) {
        fn();
    }
    const duration = performance.now() - start;
    console.log(`  ${name}: ${duration.toFixed(2)}ms (${(duration / iterations * 1000).toFixed(3)}μs/op)`);
    return duration;
}
async function runBenchmark() {
    console.log('\n=== Binary Cache Benchmark ===\n');
    // Use is-positive fixture and also create a larger dataset
    const smallMeta = loadFixture('is-positive');
    const largeMeta = createLargePackageMeta(500); // 500 versions
    const iterations = 1000;
    // Small dataset benchmark
    console.log('Small dataset (is-positive fixture):');
    const smallMetaSize = JSON.stringify(smallMeta).length;
    console.log(`  Metadata size: ${smallMetaSize} bytes JSON`);
    // JSON approach
    const jsonSmallSerialized = JSON.stringify(smallMeta);
    const jsonSmallTime = benchmark('JSON.stringify + parse', () => {
        const serialized = JSON.stringify(smallMeta);
        JSON.parse(serialized);
    }, iterations);
    // Msgpackr approach
    const msgpackSmallSerialized = packr.pack(smallMeta);
    const msgpackSmallTime = benchmark('msgpackr pack + unpack', () => {
        const serialized = packr.pack(smallMeta);
        packr.unpack(serialized);
    }, iterations);
    const smallSpeedup = (jsonSmallTime / msgpackSmallTime).toFixed(2);
    console.log(`  Speedup: ${smallSpeedup}x\n`);
    // Large dataset benchmark
    console.log('Large dataset (500 versions):');
    const largeMetaSizeMB = (JSON.stringify(largeMeta).length / 1024 / 1024).toFixed(2);
    console.log(`  Metadata size: ${largeMetaSizeMB} MB JSON`);
    // JSON approach
    const jsonLargeSerialized = JSON.stringify(largeMeta);
    const jsonLargeTime = benchmark('JSON.stringify + parse', () => {
        const serialized = JSON.stringify(largeMeta);
        JSON.parse(serialized);
    }, iterations);
    // Msgpackr approach
    const msgpackLargeSerialized = packr.pack(largeMeta);
    const msgpackLargeTime = benchmark('msgpackr pack + unpack', () => {
        const serialized = packr.pack(largeMeta);
        packr.unpack(serialized);
    }, iterations);
    const largeSpeedup = (jsonLargeTime / msgpackLargeTime).toFixed(2);
    console.log(`  Speedup: ${largeSpeedup}x`);
    const sizeReduction = (jsonLargeSerialized.length / msgpackLargeSerialized.length).toFixed(2);
    console.log(`  Size reduction: ${sizeReduction}x\n`);
    // SQLite cache benchmark
    console.log('SQLite cache (persisted):');
    const cacheDir = temporaryDirectory();
    const cache = new RegistryMetadataCache(cacheDir);
    // Benchmark writes
    const writeTime = benchmark('Cache write (large)', () => {
        cache.set('test-pkg', 'https://registry.npmjs.org/', largeMeta);
    }, iterations / 10); // Fewer iterations due to I/O
    // Benchmark reads
    const readTime = benchmark('Cache read (large)', () => {
        cache.get('test-pkg', 'https://registry.npmjs.org/');
    }, iterations);
    cache.close();
    console.log(`\nCache directory: ${cacheDir}`);
    console.log(`Database size: ${(fs.statSync(path.join(cacheDir, 'registry-metadata.db')).size / 1024).toFixed(2)} KB`);
    closeAllRegistryMetadataCaches();
    // Cleanup
    fs.rmSync(cacheDir, { recursive: true, force: true });
    console.log('\n=== Benchmark Complete ===\n');
}
runBenchmark().catch(console.error);
