use std::{env, fs::File, thread};

use memmap2::Mmap;

const HASH_MULTIPLIER: u64 = 0x517c_c1b7_2722_0a95;
const TABLE_SHIFT: u32 = 15;
const TABLE_CAPACITY: usize = 1 << TABLE_SHIFT;
const TABLE_MASK: usize = TABLE_CAPACITY - 1;
const EMPTY_BUCKET: u16 = u16::MAX;

#[inline]
fn mix_hash_word(hash: u64, word: u64) -> u64 {
    (hash.rotate_left(5) ^ word).wrapping_mul(HASH_MULTIPLIER)
}

#[inline]
fn slot_index(hash: u64) -> usize {
    ((hash ^ (hash >> 32)) as usize) & TABLE_MASK
}

#[derive(Clone, Copy)]
struct Stats {
    min: i16,
    max: i16,
    count: u32,
    sum: i64,
}

impl Stats {
    #[inline]
    fn new(temperature: i16) -> Self {
        Self {
            min: temperature,
            max: temperature,
            count: 1,
            sum: i64::from(temperature),
        }
    }

    #[inline]
    fn add_measurement(&mut self, temperature: i16) {
        self.min = self.min.min(temperature);
        self.max = self.max.max(temperature);
        self.count += 1;
        self.sum += i64::from(temperature);
    }

    #[inline]
    fn merge(&mut self, other: &Self) {
        self.min = self.min.min(other.min);
        self.max = self.max.max(other.max);
        self.count += other.count;
        self.sum += other.sum;
    }
}

#[derive(Clone, Copy)]
struct Slot<'a> {
    hash: u64,
    station: &'a [u8],
    stats: Stats,
}

struct StationTable<'a> {
    buckets: Box<[u16]>,
    entries: Vec<Slot<'a>>,
}

impl<'a> StationTable<'a> {
    fn new() -> Self {
        Self {
            buckets: vec![EMPTY_BUCKET; TABLE_CAPACITY].into_boxed_slice(),
            entries: Vec::with_capacity(10_000),
        }
    }

    #[inline]
    fn record(&mut self, station: &'a [u8], hash: u64, temperature: i16) {
        let mut index = slot_index(hash);

        loop {
            let bucket = self.buckets[index];

            if bucket == EMPTY_BUCKET {
                let entry_index = self.entries.len() as u16;
                self.buckets[index] = entry_index;
                self.entries.push(Slot {
                    hash,
                    station,
                    stats: Stats::new(temperature),
                });
                return;
            }

            let slot = &mut self.entries[bucket as usize];

            if slot.hash == hash && slot.station == station {
                slot.stats.add_measurement(temperature);
                return;
            }

            index = (index + 1) & TABLE_MASK;
        }
    }

    fn merge_from(&mut self, other: &Self) {
        for incoming in &other.entries {
            let mut index = slot_index(incoming.hash);

            loop {
                let bucket = self.buckets[index];

                if bucket == EMPTY_BUCKET {
                    let entry_index = self.entries.len() as u16;
                    self.buckets[index] = entry_index;
                    self.entries.push(*incoming);
                    break;
                }

                let slot = &mut self.entries[bucket as usize];

                if slot.hash == incoming.hash && slot.station == incoming.station {
                    slot.stats.merge(&incoming.stats);
                    break;
                }

                index = (index + 1) & TABLE_MASK;
            }
        }
    }

    fn into_entries(self) -> Vec<(&'a [u8], Stats)> {
        self.entries
            .into_iter()
            .map(|slot| (slot.station, slot.stats))
            .collect()
    }
}

#[inline]
fn parse_temperature_and_advance(raw: &[u8]) -> (i16, usize) {
    let (sign, offset) = if raw[0] == b'-' { (-1, 1) } else { (1, 0) };

    if raw[offset + 1] == b'.' {
        let tenths = i16::from(raw[offset] - b'0') * 10 + i16::from(raw[offset + 2] - b'0');
        (sign * tenths, offset + 4)
    } else {
        let tenths = i16::from(raw[offset] - b'0') * 100
            + i16::from(raw[offset + 1] - b'0') * 10
            + i16::from(raw[offset + 3] - b'0');
        (sign * tenths, offset + 5)
    }
}

#[inline]
fn find_semicolon_and_hash(bytes: &[u8]) -> (usize, u64) {
    const SEMICOLON_MASK: u64 = 0x3b3b_3b3b_3b3b_3b3b;
    const LO_BYTES: u64 = 0x0101_0101_0101_0101;
    const HI_BYTES: u64 = 0x8080_8080_8080_8080;

    let mut offset = 0;
    let mut hash = 0_u64;

    while offset + 8 <= bytes.len() {
        let word = u64::from_le_bytes(
            bytes[offset..offset + 8]
                .try_into()
                .expect("slice has eight bytes"),
        );
        let xored = word ^ SEMICOLON_MASK;
        let matches = xored.wrapping_sub(LO_BYTES) & !xored & HI_BYTES;

        if matches != 0 {
            let byte_index = (matches.trailing_zeros() >> 3) as usize;
            let separator = offset + byte_index;
            let mask = (1_u64 << (byte_index * 8)) - 1;
            let tail = (word & mask) ^ (separator as u64).wrapping_mul(HASH_MULTIPLIER);
            hash = mix_hash_word(hash, tail);
            return (separator, hash);
        }

        hash = mix_hash_word(hash, word);
        offset += 8;
    }

    let mut tail = 0_u64;
    let mut tail_index = 0;

    while offset < bytes.len() {
        let byte = bytes[offset];
        if byte == b';' {
            let tail_word = tail ^ (offset as u64).wrapping_mul(HASH_MULTIPLIER);
            hash = mix_hash_word(hash, tail_word);
            return (offset, hash);
        }
        tail |= u64::from(byte) << (tail_index * 8);
        tail_index += 1;
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

fn process_chunk(chunk: &[u8]) -> StationTable<'_> {
    let mut stations = StationTable::new();
    let mut cursor = 0;

    while cursor < chunk.len() {
        let remaining = &chunk[cursor..];
        let (separator, hash) = find_semicolon_and_hash(remaining);
        let station = &remaining[..separator];
        let (temperature, consumed) = parse_temperature_and_advance(&remaining[separator + 1..]);
        cursor += separator + 1 + consumed;

        stations.record(station, hash, temperature);
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

    let stations = thread::scope(|scope| {
        let handles: Vec<_> = chunks
            .into_iter()
            .map(|chunk| scope.spawn(move || process_chunk(chunk)))
            .collect();

        let mut merged: Option<StationTable<'_>> = None;

        for handle in handles {
            let local_table = handle.join().expect("Worker thread panicked");

            if let Some(existing) = &mut merged {
                existing.merge_from(&local_table);
            } else {
                merged = Some(local_table);
            }
        }

        merged.unwrap_or_else(StationTable::new)
    });

    let mut results = stations.into_entries();

    results.sort_unstable_by_key(|(name_a, _)| *name_a);

    print!("{{");

    for (index, (station, stats)) in results.iter().enumerate() {
        if index > 0 {
            print!(", ");
        }

        let mean = rounded_mean(stats.sum, i64::from(stats.count));
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
    fn finds_semicolon_and_matches_hash_across_paths() {
        assert_eq!(find_semicolon_and_hash(b"A;1.2\n").0, 1);
        assert_eq!(find_semicolon_and_hash(b"Hamburg;12.3\n").0, 7);
        assert_eq!(find_semicolon_and_hash(b"Dar es Salaam;26.1\n").0, 13);
        assert_eq!(
            find_semicolon_and_hash(b"Petropavlovsk-Kamchatsky;-4.8\n").0,
            24
        );
        assert_eq!(
            find_semicolon_and_hash(b"A;1.2\n"),
            find_semicolon_and_hash(b"A;1.2\nB;2.3\n")
        );
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
