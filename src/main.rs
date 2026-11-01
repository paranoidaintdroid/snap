use std::{
    collections::HashMap,
    env,
    fs::File,
    hash::{BuildHasherDefault, Hasher},
    thread,
};

use memmap2::Mmap;

const HASH_MULTIPLIER: u64 = 0x517c_c1b7_2722_0a95;

#[derive(Default)]
struct FastHasher {
    hash: u64,
}

impl FastHasher {
    #[inline]
    fn add_word(&mut self, word: u64) {
        self.hash = (self.hash.rotate_left(5) ^ word).wrapping_mul(HASH_MULTIPLIER);
    }
}

impl Hasher for FastHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }

    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);

        for chunk in &mut chunks {
            let word =
                u64::from_ne_bytes(chunk.try_into().expect("chunk must contain eight bytes"));
            self.add_word(word);
        }

        let remainder = chunks.remainder();
        let mut tail = 0_u64;

        for (index, &byte) in remainder.iter().enumerate() {
            tail |= u64::from(byte) << (index * 8);
        }

        tail ^= (bytes.len() as u64).wrapping_mul(HASH_MULTIPLIER);
        self.add_word(tail);
    }
}

type StationMap<'a> = HashMap<&'a [u8], Stats, BuildHasherDefault<FastHasher>>;

struct Stats {
    min: i32,
    max: i32,
    sum: i64,
    count: i64,
}

impl Stats {
    fn new(temperature: i32) -> Self {
        Self {
            min: temperature,
            max: temperature,
            sum: i64::from(temperature),
            count: 1,
        }
    }

    fn add_measurement(&mut self, temperature: i32) {
        self.min = self.min.min(temperature);
        self.max = self.max.max(temperature);
        self.sum += i64::from(temperature);
        self.count += 1;
    }

    fn merge(&mut self, other: &Self) {
        self.min = self.min.min(other.min);
        self.max = self.max.max(other.max);
        self.sum += other.sum;
        self.count += other.count;
    }
}

fn parse_temperature(raw: &[u8]) -> i32 {
    let mut value = 0;
    let mut is_negative = false;

    for &byte in raw {
        match byte {
            b'-' => is_negative = true,
            b'.' => {}
            b'0'..=b'9' => {
                value = value * 10 + i32::from(byte - b'0');
            }
            _ => unreachable!("invalid temperature"),
        }
    }

    if is_negative { -value } else { value }
}

fn rounded_mean(sum: i64, count: i64) -> i64 {
    (2 * sum + count).div_euclid(2 * count)
}

#[inline]
fn find_separator(line: &[u8]) -> usize {
    let length = line.len();

    if line[length - 4] == b';' {
        length - 4
    } else if line[length - 5] == b';' {
        length - 5
    } else {
        debug_assert_eq!(line[length - 6], b';');
        length - 6
    }
}

fn split_into_chunks(data: &[u8], chunk_count: usize) -> Vec<&[u8]> {
    if data.is_empty() {
        return Vec::new();
    }

    let chunk_count = chunk_count.max(1);
    let target_size = (data.len() / chunk_count).max(1);
    let mut chunks = Vec::with_capacity(chunk_count);
    let mut start = 0;

    for index in 0..chunk_count {
        if start >= data.len() {
            break;
        }

        if index + 1 == chunk_count {
            chunks.push(&data[start..]);
            break;
        }

        let tentative_end = (start + target_size).min(data.len());
        let end = match data[tentative_end..].iter().position(|&byte| byte == b'\n') {
            Some(offset) => tentative_end + offset + 1,
            None => data.len(),
        };

        chunks.push(&data[start..end]);
        start = end;
    }

    chunks
}

fn process_chunk(chunk: &[u8]) -> StationMap<'_> {
    let mut stations = StationMap::default();
    let mut start = 0;

    while start < chunk.len() {
        let end = match chunk[start..].iter().position(|&byte| byte == b'\n') {
            Some(offset) => start + offset,
            None => chunk.len(),
        };

        let line = &chunk[start..end];
        start = end + 1;

        if line.is_empty() {
            continue;
        }

        let separator = find_separator(line);
        let station = &line[..separator];
        let temperature_raw = &line[separator + 1..];
        let temperature = parse_temperature(temperature_raw);

        if let Some(stats) = stations.get_mut(station) {
            stats.add_measurement(temperature);
        } else {
            stations.insert(station, Stats::new(temperature));
        }
    }

    stations
}

fn main() {
    let path = env::args()
        .nth(1)
        .expect("Use the following syntax: snap <measurement-file>");
    let file = File::open(&path).expect("Failed to open the file");
    let mmap = unsafe { Mmap::map(&file).expect("Failed to map the file") };
    let data: &[u8] = &mmap;

    let thread_count = thread::available_parallelism()
        .map(|parallelism| parallelism.get())
        .unwrap_or(1);
    let chunks = split_into_chunks(data, thread_count);

    let mut stations = thread::scope(|scope| {
        let handles: Vec<_> = chunks
            .into_iter()
            .map(|chunk| scope.spawn(move || process_chunk(chunk)))
            .collect();

        let mut merged = StationMap::default();

        for handle in handles {
            let local_map = handle.join().expect("Worker thread panicked");

            for (station, stats) in local_map {
                if let Some(existing) = merged.get_mut(station) {
                    existing.merge(&stats);
                } else {
                    merged.insert(station, stats);
                }
            }
        }

        merged
    });

    let mut results: Vec<_> = stations.drain().collect();

    results.sort_unstable_by_key(|(name_a, _)| *name_a);

    print!("{{");

    for (index, (station, stats)) in results.iter().enumerate() {
        if index > 0 {
            print!(", ");
        }

        let mean = rounded_mean(stats.sum, stats.count);
        let station_name = std::str::from_utf8(station).expect("Invalid UTF-8 station name");

        print!(
            "{}={:.1}/{:.1}/{:.1}",
            station_name,
            f64::from(stats.min) / 10.0,
            mean as f64 / 10.0,
            f64::from(stats.max) / 10.0,
        );
    }

    println!("}}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_temperatures_as_integer_tenths() {
        assert_eq!(parse_temperature(b"0.0"), 0);
        assert_eq!(parse_temperature(b"9.9"), 99);
        assert_eq!(parse_temperature(b"12.3"), 123);
        assert_eq!(parse_temperature(b"-0.7"), -7);
        assert_eq!(parse_temperature(b"-12.3"), -123);
        assert_eq!(parse_temperature(b"99.9"), 999);
        assert_eq!(parse_temperature(b"-99.9"), -999);
    }

    #[test]
    fn rounds_mean_toward_positive_infinity() {
        assert_eq!(rounded_mean(1, 2), 1);
        assert_eq!(rounded_mean(-1, 2), 0);
        assert_eq!(rounded_mean(3, 2), 2);
        assert_eq!(rounded_mean(-3, 2), -1);
        assert_eq!(rounded_mean(28, 2), 14);
    }

    #[test]
    fn accumulates_station_statistics() {
        let mut stats = Stats::new(120);

        stats.add_measurement(340);
        stats.add_measurement(-40);

        assert_eq!(stats.min, -40);
        assert_eq!(stats.max, 340);
        assert_eq!(stats.sum, 420);
        assert_eq!(stats.count, 3);
    }

    #[test]
    fn finds_separator_from_temperature_length() {
        assert_eq!(find_separator(b"A;1.2"), 1);
        assert_eq!(find_separator(b"A;12.3"), 1);
        assert_eq!(find_separator(b"A;-1.2"), 1);
        assert_eq!(find_separator(b"A;-12.3"), 1);
    }

    #[test]
    fn splits_chunks_along_newline_boundaries() {
        let data = b"Alpha;1.0\nBeta;2.0\nGamma;3.0\nDelta;4.0\n";
        let chunks = split_into_chunks(data, 3);

        let rejoined: Vec<u8> = chunks.iter().copied().flatten().copied().collect();
        assert_eq!(rejoined, data);

        for chunk in chunks {
            assert!(!chunk.is_empty());
            assert_eq!(chunk.last(), Some(&b'\n'));
        }
    }
}
