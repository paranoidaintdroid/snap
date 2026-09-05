# Snap 1BRC benchmarks

Dataset: 1,000,000,000 rows, approximately 15 GB.

| Version | Implementation | Time | Throughput | Time/row | Peak RSS |
|---|---|---:|---:|---:|---:|
| v0 | `BufReader::lines`, `f64`, `HashMap<String, Stats>` | 366.72 s | 2.73 M rows/s | 366.72 ns | 3.65 MiB |
| v1 | Manual byte parser, integer tenths, exact rounding | 315.51 s avg | 3.17 M rows/s | 315.51 ns | 3.40 MiB |
| v2 | Borrowed station lookup; allocate only new station keys | 239.32 s | 4.18 M rows/s | 239.32 ns | 3.39 MiB |
| v3 | Reused one `String` buffer with `read_line` | 193.22 s | 5.18 M rows/s | 193.22 ns | 3.40 MiB |


> v0 was not fully correct: floating-point formatting emitted
> negative zero and rounded the exact mean -0.15°C for İliç to -0.2°C.
> v1 correctly produces -0.1°C using round-toward-positive.