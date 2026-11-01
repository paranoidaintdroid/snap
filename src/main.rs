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

#[inline]
fn parse_temperature_and_advance(raw: &[u8]) -> (i32, usize) {
    let (sign, offset) = if raw[0] == b'-' { (-1, 1) } else { (1, 0) };

    if raw[offset + 1] == b'.' {
        let tenths = i32::from(raw[offset] - b'0') * 10 + i32::from(raw[offset + 2] - b'0');
        (sign * tenths, offset + 4)
    } else {
        let tenths = i32::from(raw[offset] - b'0') * 100
            + i32::from(raw[offset + 1] - b'0') * 10
            + i32::from(raw[offset + 3] - b'0');
        (sign * tenths, offset + 5)
    }
}

#[inline]
fn find_semicolon(bytes: &[u8]) -> usize {
    const SEMICOLON_MASK: u64 = 0x3b3b_3b3b_3b3b_3b3b;
    const LO_BYTES: u64 = 0x0101_0101_0101_0101;
    const HI_BYTES: u64 = 0x8080_8080_8080_8080;

    let mut offset = 0;

    while offset + 8 <= bytes.len() {
        let word = u64::from_le_bytes(
            bytes[offset..offset + 8]
                .try_into()
                .expect("slice has eight bytes"),
        );
        let xored = word ^ SEMICOLON_MASK;
        let matches = xored.wrapping_sub(LO_BYTES) & !xored & HI_BYTES;

        if matches != 0 {
            return offset + (matches.trailing_zeros() as usize >> 3);
        }

        offset += 8;
    }

    while offset < bytes.len() {
        if bytes[offset] == b';' {
            return offset;
        }
        offset += 1;
    }

    unreachable!("missing semicolon separator")
}

fn rounded_mean(sum: i64, count: i64) -> i64 {
    (2 * sum + count).div_euclid(2 * count)
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
    let mut cursor = 0;

    while cursor < chunk.len() {
        let remaining = &chunk[cursor..];
        let separator = find_semicolon(remaining);
        let station = &remaining[..separator];
        let (temperature, consumed) = parse_temperature_and_advance(&remaining[separator + 1..]);
        cursor += separator + 1 + consumed;

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
        assert_eq!(parse_temperature_and_advance(b"0.0\n"), (0, 4));
        assert_eq!(parse_temperature_and_advance(b"9.9\n"), (99, 4));
        assert_eq!(parse_temperature_and_advance(b"12.3\n"), (123, 5));
        assert_eq!(parse_temperature_and_advance(b"-0.7\n"), (-7, 5));
        assert_eq!(parse_temperature_and_advance(b"-12.3\n"), (-123, 6));
        assert_eq!(parse_temperature_and_advance(b"99.9\n"), (999, 5));
        assert_eq!(parse_temperature_and_advance(b"-99.9\n"), (-999, 6));
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
    fn finds_semicolon_across_short_and_long_names() {
        assert_eq!(find_semicolon(b"A;1.2\n"), 1);
        assert_eq!(find_semicolon(b"Hamburg;12.3\n"), 7);
        assert_eq!(find_semicolon(b"Dar es Salaam;26.1\n"), 13);
        assert_eq!(find_semicolon(b"Petropavlovsk-Kamchatsky;-4.8\n"), 24);
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
