"""Compare separately built repeated_range_picks examples."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess

parser = argparse.ArgumentParser()
parser.add_argument('--base', required=True)
parser.add_argument('--candidate', required=True)
parser.add_argument('--pairs', type=int, default=12)
args = parser.parse_args()
results = dict(platform=platform.platform(), machine=platform.machine(),
               cpus=os.cpu_count(), pairs=args.pairs, unit='ms',
               sha256={s: hashlib.sha256(Path(getattr(args, s)).read_bytes()).hexdigest()
                       for s in ['base', 'candidate']}, cases=[])
for mode in ['raw', 'file', 'raw-age', 'file-age']:
    for count in [1, 100]:
        row = dict(mode=mode, picks=count, base=[], candidate=[])
        for pair in range(args.pairs):
            order = ['base', 'candidate'] if pair % 2 == 0 else ['candidate', 'base']
            for side in order:
                row[side].append(float(subprocess.check_output(
                    [getattr(args, side), mode, str(count)], text=True)))
        base, candidate = (statistics.median(row[s]) for s in ['base', 'candidate'])
        row.update(base_median=base, candidate_median=candidate,
                   reduction_pct=100 * (1 - candidate / base))
        results['cases'].append(row)
results['load_after'] = os.getloadavg()
print(json.dumps(results, indent=2))
