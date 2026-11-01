# Snap 1BRC benchmarks

Dataset: 1,000,000,000 rows, approximately 15 GB.

| Version | Implementation | Time | Throughput | Time/row | Peak RSS |
|---|---|---:|---:|---:|---:|
| v0 | `BufReader::lines`, `f64`, `HashMap<String, Stats>` | 366.72 s | 2.73 M rows/s | 366.72 ns | 3.65 MiB |
| v1 | Manual byte parser, integer tenths, exact rounding | 315.51 s avg | 3.17 M rows/s | 315.51 ns | 3.40 MiB |
| v2 | Borrowed station lookup; allocate only new station keys | 239.32 s | 4.18 M rows/s | 239.32 ns | 3.39 MiB |
| v3 | Reused one `String` buffer with `read_line` | 193.22 s | 5.18 M rows/s | 193.22 ns | 3.40 MiB |
| v4 | Reused byte buffer; skipped repeated UTF-8 validation for trusted input | 184.09 s | 5.43 M rows/s | 184.09 ns | 3.45 MiB |
| v5 | Word-at-a-time deterministic station-name hasher | 155.73 s avg | 6.42 M rows/s | 155.73 ns | 3.36 MiB |
| v6 | Derived separator position from fixed-width temperature suffix | 143.56 s | 6.97 M rows/s | 143.56 ns | 3.30 MiB |

> v0 was not fully correct: floating-point formatting emitted
> negative zero and rounded the exact mean -0.15°C for İliç to -0.2°C.
> v1 correctly produces -0.1°C using round-toward-positive.

- Rejected raw-byte `read_until` version: 193.72–198.21 s.
- Adding a 1 MiB `BufReader` produced 197.38 s.
- Neither consistently improved on v3 at 193.22 s.
- Rejected custom FNV-1a hasher: 209.50 s.
- Rejected direct indexed temperature parser: 194.33–220.03 s.
- On the cached 50M-row profiling dataset, the UTF-8 fast path reduced time from 11.65 s to 9.37 s.
- v5 full-dataset runs: 159.33 s and 152.12 s; the table reports their 155.73 s mean.
- On the 50M-row profiling dataset, memory-mapping the input and scanning for newlines directly reduced time from 7.67–8.85 s (two runs) to 5.11–5.85 s (three runs), single-threaded, output identical. Measured once per build, not a controlled benchmark.
