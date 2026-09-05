use std::{
    collections::HashMap,
    env,
    fs::File,
    io::{BufRead, BufReader},
};

struct Stats {
    min: i32,
    max: i32,
    sum: i64,
    count: i64,
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
    let reader = BufReader::new(file);

    let mut stations: HashMap<String, Stats> = HashMap::new();

    for line_r in reader.lines() {
        let line = line_r.expect("Error reading a line");
        //println!("{line}");

        let (station, temperature_raw) =
            line.split_once(';').expect("Failed to extract measurement");

        let temperature = parse_temperature(temperature_raw);

        //println!("station={station:?}, temperature={temperature}");

        let stats = stations.entry(station.to_owned()).or_insert(Stats {
            min: i32::MAX,
            max: i32::MIN,
            sum: 0,
            count: 0,
        });

        stats.min = stats.min.min(temperature);
        stats.max = stats.max.max(temperature);
        stats.sum += i64::from(temperature);
        stats.count += 1;
    }

    // for (station, stats) in &stations {
    //         let mean = stats.sum / stats.count as f64;
    //         println!(
    //             "{station}: min={}, mean={}, max={}, count={}, sum={}",
    //             stats.min, mean, stats.max, stats.count, stats.sum
    //         );
    //     }

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
}
