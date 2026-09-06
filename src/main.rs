use std::{
    collections::HashMap,
    env,
    fs::File,
    hash::{BuildHasherDefault, Hasher},
    io::{BufRead, BufReader},
};

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

type StationMap = HashMap<String, Stats, BuildHasherDefault<FastHasher>>;

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
}

fn parse_temperature(raw: &str) -> i32 {
    let mut value = 0;
    let mut is_negative = false;

    for &byte in raw.as_bytes() {
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

fn main() {
    let path = env::args()
        .nth(1)
        .expect("Use the following syntax: snap <measurement-file>");
    let file = File::open(&path).expect("Failed to open the file");
    let mut reader = BufReader::new(file);

    let mut stations = StationMap::default();

    let mut line = Vec::new();

    loop {
        line.clear();

        let bytes_read = reader
            .read_until(b'\n', &mut line)
            .expect("Error reading a line");

        if bytes_read == 0 {
            break;
        }

        if line.last() == Some(&b'\n') {
            line.pop();
        }

        debug_assert!(std::str::from_utf8(&line).is_ok());

        // SAFETY: The official 1BRC input is guaranteed to contain valid UTF-8.
        // The Python generator writes UTF-8 station names and ASCII measurements.
        let measurement = unsafe { std::str::from_utf8_unchecked(&line) };

        let (station, temperature_raw) = measurement
            .split_once(';')
            .expect("Failed to extract measurement");

        let temperature = parse_temperature(temperature_raw);

        if let Some(stats) = stations.get_mut(station) {
            stats.add_measurement(temperature);
        } else {
            stations.insert(station.to_owned(), Stats::new(temperature));
        }
    }

    let mut results: Vec<_> = stations.iter().collect();

    results.sort_unstable_by_key(|(name_a, _)| *name_a);

    print!("{{");

    for (index, (station, stats)) in results.iter().enumerate() {
        if index > 0 {
            print!(", ");
        }

        let mean = rounded_mean(stats.sum, stats.count);

        print!(
            "{}={:.1}/{:.1}/{:.1}",
            station,
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
        assert_eq!(parse_temperature("0.0"), 0);
        assert_eq!(parse_temperature("9.9"), 99);
        assert_eq!(parse_temperature("12.3"), 123);
        assert_eq!(parse_temperature("-0.7"), -7);
        assert_eq!(parse_temperature("-12.3"), -123);
        assert_eq!(parse_temperature("99.9"), 999);
        assert_eq!(parse_temperature("-99.9"), -999);
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
}
