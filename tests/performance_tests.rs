use rust_decimal::Decimal;
use std::time::Instant;
use uuid::Uuid;

use settlement_engine::cache::{BalanceCache, CacheStats};
use settlement_engine::config::CacheSettings;
use settlement_engine::models::{AccountBalance, TransactionRecord, TransactionType};
use settlement_engine::observability::LatencyTimer;

#[test]
fn test_cache_stats_concurrent_access() {
    use std::sync::Arc;
    use std::thread;

    let stats = Arc::new(CacheStats::new());
    let mut handles = vec![];

    for _ in 0..10 {
        let stats_clone = stats.clone();
        handles.push(thread::spawn(move || {
            for _ in 0..1000 {
                stats_clone.record_hit();
                stats_clone.record_miss();
            }
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }

    assert_eq!(stats.get_hits(), 10000);
    assert_eq!(stats.get_misses(), 10000);
    assert!((stats.hit_rate() - 0.5).abs() < 0.01);
}

#[test]
fn test_cache_stats_hit_rate_edge_cases() {
    let stats = CacheStats::new();
    
    assert_eq!(stats.hit_rate(), 0.0);

    stats.record_hit();
    assert_eq!(stats.hit_rate(), 1.0);

    stats.record_miss();
    assert_eq!(stats.hit_rate(), 0.5);

    for _ in 0..98 {
        stats.record_hit();
    }
    assert!((stats.hit_rate() - 0.99).abs() < 0.01);
}

#[test]
fn test_balance_creation_performance() {
    let start = Instant::now();
    let iterations = 10000;

    for _ in 0..iterations {
        let _balance = AccountBalance::new(Uuid::new_v4(), "USD".to_string());
    }

    let elapsed = start.elapsed();
    let per_op = elapsed.as_nanos() / iterations as u128;
    
    println!("Balance creation: {} ns/op", per_op);
    assert!(per_op < 10_000, "Balance creation too slow: {} ns/op", per_op);
}

#[test]
fn test_balance_calculations_performance() {
    let mut balance = AccountBalance::with_available_balance(
        Uuid::new_v4(),
        "USD".to_string(),
        Decimal::from(1_000_000),
    );
    balance.pending_balance = Decimal::from(50_000);
    balance.reserved_balance = Decimal::from(25_000);

    let start = Instant::now();
    let iterations = 100_000;

    for _ in 0..iterations {
        let _ = balance.total_balance();
        let _ = balance.usable_balance();
        let _ = balance.has_sufficient_funds(Decimal::from(1000));
    }

    let elapsed = start.elapsed();
    let per_op = elapsed.as_nanos() / (iterations * 3) as u128;
    
    println!("Balance calculation: {} ns/op", per_op);
    assert!(per_op < 1_000, "Balance calculation too slow: {} ns/op", per_op);
}

#[test]
fn test_transaction_creation_performance() {
    let source_id = Uuid::new_v4();
    let dest_id = Uuid::new_v4();

    let start = Instant::now();
    let iterations = 10000;

    for i in 0..iterations {
        let _tx = TransactionRecord::new(
            format!("EXT-{}", i),
            TransactionType::Payment,
            source_id,
            dest_id,
            Decimal::from(1000),
            "USD".to_string(),
            Decimal::ZERO,
            format!("idem-{}", i),
        );
    }

    let elapsed = start.elapsed();
    let per_op = elapsed.as_nanos() / iterations as u128;
    
    println!("Transaction creation: {} ns/op", per_op);
    assert!(per_op < 50_000, "Transaction creation too slow: {} ns/op", per_op);
}

#[test]
fn test_hashmap_aggregation_performance() {
    use std::collections::HashMap;
    
    let participants: Vec<Uuid> = (0..10).map(|_| Uuid::new_v4()).collect();
    
    let start = Instant::now();
    let iterations = 100;

    for _ in 0..iterations {
        let mut positions: HashMap<Uuid, Decimal> = HashMap::new();
        
        for i in 0..1000 {
            let from_idx = i % participants.len();
            let to_idx = (i + 1) % participants.len();
            let amount = Decimal::from((i % 1000) as i64 + 100);
            
            *positions.entry(participants[from_idx]).or_insert(Decimal::ZERO) -= amount;
            *positions.entry(participants[to_idx]).or_insert(Decimal::ZERO) += amount;
        }
        
        std::hint::black_box(positions);
    }

    let elapsed = start.elapsed();
    let per_op_us = elapsed.as_micros() / iterations as u128;
    
    println!("HashMap aggregation 1000 txs: {} us/op", per_op_us);
    assert!(per_op_us < 10_000, "HashMap aggregation too slow: {} us/op", per_op_us);
}

#[test]
fn test_large_batch_simulation() {
    use std::collections::HashMap;
    
    let participants: Vec<Uuid> = (0..100).map(|_| Uuid::new_v4()).collect();
    
    let start = Instant::now();
    let mut positions: HashMap<Uuid, Decimal> = HashMap::new();
    
    for i in 0..10000 {
        let from_idx = i % participants.len();
        let to_idx = (i + 1) % participants.len();
        let amount = Decimal::from((i % 1000) as i64 + 100);
        
        *positions.entry(participants[from_idx]).or_insert(Decimal::ZERO) -= amount;
        *positions.entry(participants[to_idx]).or_insert(Decimal::ZERO) += amount;
    }

    let elapsed = start.elapsed();
    
    println!("Large batch simulation 10000 txs: {} ms", elapsed.as_millis());
    println!("Unique positions: {}", positions.len());
    
    assert!(elapsed.as_millis() < 100, "Large batch simulation too slow: {} ms", elapsed.as_millis());
}

#[test]
fn test_latency_timer_overhead() {
    let start = Instant::now();
    let iterations = 100_000;

    for _ in 0..iterations {
        let timer = LatencyTimer::new();
        let _ = timer.elapsed_ms();
    }

    let elapsed = start.elapsed();
    let per_op = elapsed.as_nanos() / iterations as u128;
    
    println!("LatencyTimer overhead: {} ns/op", per_op);
    assert!(per_op < 1_000, "LatencyTimer overhead too high: {} ns/op", per_op);
}

#[test]
fn test_uuid_generation_performance() {
    let start = Instant::now();
    let iterations = 100_000;

    for _ in 0..iterations {
        let _ = Uuid::new_v4();
    }

    let elapsed = start.elapsed();
    let per_op = elapsed.as_nanos() / iterations as u128;
    
    println!("UUID generation: {} ns/op", per_op);
    assert!(per_op < 10_000, "UUID generation too slow: {} ns/op", per_op);
}

#[test]
fn test_decimal_arithmetic_performance() {
    let a = Decimal::from(123456789);
    let b = Decimal::from(987654321);

    let start = Instant::now();
    let iterations = 1_000_000;

    for _ in 0..iterations {
        let _ = a + b;
        let _ = a - b;
        let _ = a * b;
        let _ = a < b;
    }

    let elapsed = start.elapsed();
    let per_op = elapsed.as_nanos() / (iterations * 4) as u128;
    
    println!("Decimal arithmetic: {} ns/op", per_op);
    assert!(per_op < 500, "Decimal arithmetic too slow: {} ns/op", per_op);
}

#[test]
fn test_cache_key_generation_performance() {
    let account_id = Uuid::new_v4();

    let start = Instant::now();
    let iterations = 100_000;

    for _ in 0..iterations {
        let key = format!("settlement:balance:{}:USD", account_id);
        std::hint::black_box(key);
    }

    let elapsed = start.elapsed();
    let per_op = elapsed.as_nanos() / iterations as u128;
    
    println!("Cache key generation: {} ns/op", per_op);
    assert!(per_op < 5_000, "Cache key generation too slow: {} ns/op", per_op);
}

#[test]
fn test_string_formatting_performance() {
    let account_id = Uuid::new_v4();

    let start = Instant::now();
    let iterations = 100_000;

    for _ in 0..iterations {
        let key = format!("settlement:balance:{}:USD", account_id);
        std::hint::black_box(key);
    }

    let elapsed = start.elapsed();
    let per_op = elapsed.as_nanos() / iterations as u128;
    
    println!("String formatting: {} ns/op", per_op);
    assert!(per_op < 5_000, "String formatting too slow: {} ns/op", per_op);
}

#[test]
fn test_netting_efficiency_simulation() {
    use std::collections::HashMap;
    
    let participants: Vec<Uuid> = (0..10).map(|_| Uuid::new_v4()).collect();
    let mut positions: HashMap<Uuid, Decimal> = HashMap::new();
    let mut gross_total = Decimal::ZERO;

    for i in 0..1000 {
        let from_idx = i % participants.len();
        let to_idx = (i + 1) % participants.len();
        let amount = Decimal::from((i % 1000) as i64 + 100);
        
        gross_total += amount;
        *positions.entry(participants[from_idx]).or_insert(Decimal::ZERO) -= amount;
        *positions.entry(participants[to_idx]).or_insert(Decimal::ZERO) += amount;
    }

    let net_total: Decimal = positions.values().map(|v| v.abs()).sum();
    
    let efficiency = if gross_total > Decimal::ZERO {
        (Decimal::ONE - (net_total / gross_total)) * Decimal::from(100)
    } else {
        Decimal::ZERO
    };

    println!("Netting efficiency: {}%", efficiency);
    println!("Gross total: {}", gross_total);
    println!("Net total: {}", net_total);
    println!("Positions: {}", positions.len());
}
