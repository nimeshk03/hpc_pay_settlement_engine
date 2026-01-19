use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::time::Duration;
use uuid::Uuid;

use settlement_engine::cache::CacheStats;
use settlement_engine::models::{AccountBalance, TransactionRecord, TransactionType};
use settlement_engine::observability::LatencyTimer;

fn benchmark_netting_simulation(c: &mut Criterion) {
    let mut group = c.benchmark_group("netting");
    group.measurement_time(Duration::from_secs(10));

    for size in [100, 1000, 10000].iter() {
        group.bench_with_input(
            BenchmarkId::new("hashmap_aggregation", size),
            size,
            |b, &size| {
                let participants: Vec<Uuid> = (0..10).map(|_| Uuid::new_v4()).collect();

                b.iter(|| {
                    let mut positions: HashMap<Uuid, Decimal> = HashMap::new();
                    for i in 0..size {
                        let from_idx = i % participants.len();
                        let to_idx = (i + 1) % participants.len();
                        let amount = Decimal::from((i % 1000) as i64 + 100);
                        
                        *positions.entry(participants[from_idx]).or_insert(Decimal::ZERO) -= amount;
                        *positions.entry(participants[to_idx]).or_insert(Decimal::ZERO) += amount;
                    }
                    black_box(positions)
                });
            },
        );
    }

    group.finish();
}

fn benchmark_balance_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("balance");

    group.bench_function("create_balance", |b| {
        b.iter(|| {
            let balance = AccountBalance::new(
                black_box(Uuid::new_v4()),
                black_box("USD".to_string()),
            );
            black_box(balance)
        });
    });

    group.bench_function("balance_with_initial", |b| {
        b.iter(|| {
            let balance = AccountBalance::with_available_balance(
                black_box(Uuid::new_v4()),
                black_box("USD".to_string()),
                black_box(Decimal::from(10000)),
            );
            black_box(balance)
        });
    });

    group.bench_function("has_sufficient_funds", |b| {
        let balance = AccountBalance::with_available_balance(
            Uuid::new_v4(),
            "USD".to_string(),
            Decimal::from(10000),
        );

        b.iter(|| {
            let result = balance.has_sufficient_funds(black_box(Decimal::from(500)));
            black_box(result)
        });
    });

    group.bench_function("total_balance_calculation", |b| {
        let mut balance = AccountBalance::with_available_balance(
            Uuid::new_v4(),
            "USD".to_string(),
            Decimal::from(10000),
        );
        balance.pending_balance = Decimal::from(500);
        balance.reserved_balance = Decimal::from(200);

        b.iter(|| {
            let total = balance.total_balance();
            let usable = balance.usable_balance();
            black_box((total, usable))
        });
    });

    group.finish();
}

fn benchmark_transaction_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("transaction");

    group.bench_function("create_transaction_record", |b| {
        let source_id = Uuid::new_v4();
        let dest_id = Uuid::new_v4();

        b.iter(|| {
            let tx = TransactionRecord::new(
                black_box("EXT-001".to_string()),
                black_box(TransactionType::Payment),
                black_box(source_id),
                black_box(dest_id),
                black_box(Decimal::from(1000)),
                black_box("USD".to_string()),
                black_box(Decimal::ZERO),
                black_box("idem-001".to_string()),
            );
            black_box(tx)
        });
    });

    group.bench_function("transaction_with_fee", |b| {
        let source_id = Uuid::new_v4();
        let dest_id = Uuid::new_v4();

        b.iter(|| {
            let tx = TransactionRecord::new(
                black_box("EXT-001".to_string()),
                black_box(TransactionType::Payment),
                black_box(source_id),
                black_box(dest_id),
                black_box(Decimal::from(1000)),
                black_box("USD".to_string()),
                black_box(Decimal::from(10)),
                black_box("idem-002".to_string()),
            );
            black_box(tx)
        });
    });

    group.finish();
}

fn benchmark_cache_stats(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_stats");

    group.bench_function("record_hit", |b| {
        let stats = CacheStats::new();
        b.iter(|| {
            stats.record_hit();
        });
    });

    group.bench_function("hit_rate_calculation", |b| {
        let stats = CacheStats::new();
        for _ in 0..1000 {
            stats.record_hit();
        }
        for _ in 0..100 {
            stats.record_miss();
        }

        b.iter(|| {
            let rate = stats.hit_rate();
            black_box(rate)
        });
    });

    group.finish();
}

fn benchmark_latency_timer(c: &mut Criterion) {
    let mut group = c.benchmark_group("latency_timer");

    group.bench_function("create_and_elapsed", |b| {
        b.iter(|| {
            let timer = LatencyTimer::new();
            let elapsed = timer.elapsed_ms();
            black_box(elapsed)
        });
    });

    group.finish();
}

fn benchmark_uuid_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("uuid");

    group.bench_function("generate_v4", |b| {
        b.iter(|| {
            let id = Uuid::new_v4();
            black_box(id)
        });
    });

    group.bench_function("to_string", |b| {
        let id = Uuid::new_v4();
        b.iter(|| {
            let s = id.to_string();
            black_box(s)
        });
    });

    group.finish();
}

fn benchmark_decimal_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("decimal");

    group.bench_function("addition", |b| {
        let a = Decimal::from(12345);
        let b_val = Decimal::from(67890);
        b.iter(|| {
            let result = a + b_val;
            black_box(result)
        });
    });

    group.bench_function("multiplication", |b| {
        let a = Decimal::from(12345);
        let b_val = Decimal::from(67890);
        b.iter(|| {
            let result = a * b_val;
            black_box(result)
        });
    });

    group.bench_function("comparison", |b| {
        let a = Decimal::from(12345);
        let b_val = Decimal::from(67890);
        b.iter(|| {
            let result = a < b_val;
            black_box(result)
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    benchmark_netting_simulation,
    benchmark_balance_operations,
    benchmark_transaction_creation,
    benchmark_cache_stats,
    benchmark_latency_timer,
    benchmark_uuid_operations,
    benchmark_decimal_operations,
);

criterion_main!(benches);
