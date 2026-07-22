//! Reproducible end-to-end throughput benchmarks using a representative 10 Hz VBO recording.

use std::fmt::Write;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use racelogic_vbo::{Parser, Telemetry};

const SAMPLE_COUNT: usize = 100_000;
const SAMPLE_RATE_HZ: usize = 10;

fn representative_vbo(rows: usize) -> String {
    let mut input = String::with_capacity(rows * 90);
    input.push_str(
        "File created on 2026-01-01\n\
         [header]\n\
         vehicle: benchmark\n\
         [channel units]\n\
         hhmmss degmin degmin kmh m deg ms 1\n\
         [column names]\n\
         time lat long velocity altitude heading vertical_velocity sats\n\
         [data]\n",
    );

    for row in 0..rows {
        let elapsed_tenths = row;
        let whole_seconds = 12 * 60 * 60 + elapsed_tenths / SAMPLE_RATE_HZ;
        let hours = whole_seconds / 3_600;
        let minutes = whole_seconds / 60 % 60;
        let seconds = whole_seconds % 60;
        let tenths = elapsed_tenths % SAMPLE_RATE_HZ;
        let progress = f64::from(u32::try_from(row).unwrap_or(u32::MAX));
        let latitude = 5_123.123_4 + progress * 0.000_001;
        let longitude = 0.123_4 + progress * 0.000_002;
        let speed = 80.0 + (progress / 50.0).sin() * 20.0;
        let altitude = 100.0 + (progress / 1_000.0).sin() * 15.0;
        let heading = (progress * 0.15).rem_euclid(360.0);
        let vertical_velocity = (progress / 100.0).sin();

        writeln!(
            input,
            "{hours:02}{minutes:02}{seconds:02}.{tenths} {latitude:.6} {longitude:.6} \
             {speed:.3} {altitude:.3} {heading:.2} {vertical_velocity:.3} 137"
        )
        .expect("writing to a String cannot fail");
    }
    input
}

fn parser_benchmarks(c: &mut Criterion) {
    let input = representative_vbo(SAMPLE_COUNT);
    let mut group = c.benchmark_group("parse");
    group.throughput(Throughput::Bytes(
        u64::try_from(input.len()).unwrap_or(u64::MAX),
    ));
    group.bench_with_input(
        BenchmarkId::new("strict_in_memory", SAMPLE_COUNT),
        &input,
        |bench, input| {
            bench.iter(|| {
                Parser::default()
                    .parse_str(black_box(input))
                    .expect("generated benchmark input must parse")
            });
        },
    );
    group.finish();
}

fn telemetry_benchmarks(c: &mut Criterion) {
    let input = representative_vbo(SAMPLE_COUNT);
    let session = Parser::default()
        .parse_str(&input)
        .expect("generated benchmark input must parse");
    let mut group = c.benchmark_group("telemetry");
    group.throughput(Throughput::Elements(
        u64::try_from(SAMPLE_COUNT).unwrap_or(u64::MAX),
    ));
    group.bench_function("analyse/100000_samples", |bench| {
        bench.iter(|| black_box(session.analyse()));
    });
    group.finish();
}

criterion_group!(benches, parser_benchmarks, telemetry_benchmarks);
criterion_main!(benches);
