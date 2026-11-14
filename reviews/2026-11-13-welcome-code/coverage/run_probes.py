#!/usr/bin/env python3
"""Run reviewer-only probes against the snapshot and saved baseline production source."""
from pathlib import Path
import os
import subprocess

base = Path(__file__).resolve().parent
production = base / 'work/qdl/src/lib.rs'
current = (base.parent / 'source/qdl/src/lib.rs').read_bytes()
baseline = (base / 'baseline-lib.rs').read_bytes()
try:
    for name, contents, trim in [
        ('current', current, '1'),
        ('baseline-exact', baseline, '0'),
    ]:
        production.write_bytes(contents)
        with (base / f'{name}-probe-tests.log').open('w') as log:
            result = subprocess.run(
                ['cargo', 'test', '-p', 'qdl', '--all-features', '--test',
                 'review_probe', '--locked', '--offline', '--', '--nocapture'],
                cwd=base / 'work', stdout=log, stderr=subprocess.STDOUT,
                env=dict(os.environ, REVIEW_EXPECT_TRIM=trim),
            )
        print(f'{name}: {result.returncode}')
        if result.returncode:
            raise SystemExit(result.returncode)
finally:
    production.write_bytes(current)
