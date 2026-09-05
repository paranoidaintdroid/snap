use std::{
    collections::HashMap,
    env,
    fs::File,
    io::{BufRead, BufReader},
};

struct Stats {
    min: f64,
    max: f64,
    sum: f64,
    count: u64,
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
        let temperature: f64 = temperature_raw
            .parse()
            .expect("Failed to parse temperature");

        //println!("station={station:?}, temperature={temperature}");

        let stats = stations.entry(station.to_owned()).or_insert(Stats {
            min: f64::INFINITY,
            max: f64::NEG_INFINITY,
            sum: 0.0,
            count: 0,
        });

        stats.min = stats.min.min(temperature);
        stats.max = stats.max.max(temperature);
        stats.sum += temperature;
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

        let mean = stats.sum / stats.count as f64;

        print!("{}={:.1}/{:.1}/{:.1}", station, stats.min, mean, stats.max);
    }

    println!("}}");
}
