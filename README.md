# snap

Rust implementation of the [One Billion Row Challenge (1BRC)](https://github.com/gunnarmorling/1brc), built iteratively from a naive `BufReader` + `HashMap<String, Stats>` baseline down to a multi-threaded, memory-mapped SWAR parser with a custom cache-resident index table.

## Problem

Given a text file of 1,000,000,000 temperature measurements (`<station>;<temperature>\n`, ~15.9 GB), compute the minimum, mean (rounded toward positive infinity to one decimal place), and maximum temperature for each weather station, sorted alphabetically by station name:

```text
{Abha=-23.0/18.0/59.2, Abidjan=-16.2/26.0/67.3, ...}
```

## Progression

Full benchmark notes and rejected experiments are tracked in [BENCHMARKS.md](BENCHMARKS.md).

| Version | Change | Time (1B rows) | Throughput | Speedup vs v0 |
|---|---|---:|---:|---:|
| v0 | `BufReader::lines`, `f64`, `HashMap<String, Stats>` | 366.72 s | 2.73 M rows/s | 1.0x |
| v1 | Manual byte parser, integer tenths, exact rounding | 315.51 s | 3.17 M rows/s | 1.2x |
| v2 | Borrowed station lookup; allocate only new station keys | 239.32 s | 4.18 M rows/s | 1.5x |
| v3 | Reused single `String` buffer with `read_line` | 193.22 s | 5.18 M rows/s | 1.9x |
| v4 | Reused byte buffer; skipped repeated UTF-8 validation | 184.09 s | 5.43 M rows/s | 2.0x |
| v5 | Word-at-a-time deterministic station-name hasher | 155.73 s | 6.42 M rows/s | 2.4x |
| v6 | Derived separator position from fixed-width temperature suffix | 143.56 s | 6.97 M rows/s | 2.6x |
| v7 | Memory-mapped input split across threads with borrowed station keys | 78.74 s | 12.70 M rows/s | 4.7x |
| v8 | SWAR semicolon search with direct temperature-and-newline advance | 23.30 s | 42.92 M rows/s | 15.7x |
| v9 | Fused SWAR semicolon + hash with compact `u16` bucket index table | 17.42 s | 57.41 M rows/s | 21.1x |
| v10 | Branchless temperature parser + bijective short-key equality | 14.39 s | 69.49 M rows/s | 25.5x |

## Usage

Generate the dataset (requires Python 3):

```bash
python3 src/main/python/create_measurements.py 1000000000
```

Build and run:

```bash
cargo build --release
./target/release/snap data/measurements.txt > result.txt
```

Run tests:

```bash
cargo test --release
```
