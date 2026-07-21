use criterion::{black_box, criterion_group, criterion_main, Criterion};

// ── 基准测试：字符串分配性能 ─────────────────────────────────────────────────

fn bench_string_to_string(c: &mut Criterion) {
    let data = "This is a test string for benchmarking string allocation performance";

    c.bench_function("string_to_string", |b| {
        b.iter(|| {
            let _s: String = black_box(data).to_string();
            black_box(_s);
        });
    });
}

fn bench_string_borrow(c: &mut Criterion) {
    let data = "This is a test string for benchmarking string allocation performance";

    c.bench_function("string_borrow", |b| {
        b.iter(|| {
            let _s: &str = black_box(data);
            black_box(_s);
        });
    });
}

fn bench_string_clone(c: &mut Criterion) {
    let data = "This is a test string for benchmarking string allocation performance".to_string();

    c.bench_function("string_clone", |b| {
        b.iter(|| {
            let _s = black_box(&data).clone();
            black_box(_s);
        });
    });
}

fn bench_string_arc_clone(c: &mut Criterion) {
    let data = std::sync::Arc::new(
        "This is a test string for benchmarking string allocation performance".to_string(),
    );

    c.bench_function("string_arc_clone", |b| {
        b.iter(|| {
            let _s = black_box(&data).clone();
            black_box(_s);
        });
    });
}

// ── 基准测试：Vec<String> vs Vec<&str> ───────────────────────────────────────

fn bench_vec_string_allocation(c: &mut Criterion) {
    let items = vec![
        "item1", "item2", "item3", "item4", "item5", "item6", "item7", "item8", "item9", "item10",
    ];

    c.bench_function("vec_string_allocation", |b| {
        b.iter(|| {
            let v: Vec<String> = items.iter().map(|s| s.to_string()).collect();
            black_box(v);
        });
    });
}

fn bench_vec_str_borrow(c: &mut Criterion) {
    let items = vec![
        "item1", "item2", "item3", "item4", "item5", "item6", "item7", "item8", "item9", "item10",
    ];

    c.bench_function("vec_str_borrow", |b| {
        b.iter(|| {
            let v: Vec<&str> = items.iter().copied().collect();
            black_box(v);
        });
    });
}

// ── 基准测试：format! vs concat ─────────────────────────────────────────────

fn bench_format_macro(c: &mut Criterion) {
    let prefix = "prefix";
    let suffix = "suffix";

    c.bench_function("format_macro", |b| {
        b.iter(|| {
            let s = format!("{}{}", black_box(prefix), black_box(suffix));
            black_box(s);
        });
    });
}

fn bench_string_concat(c: &mut Criterion) {
    let prefix = "prefix";
    let suffix = "suffix";

    c.bench_function("string_concat", |b| {
        b.iter(|| {
            let mut s = String::with_capacity(prefix.len() + suffix.len());
            s.push_str(black_box(prefix));
            s.push_str(black_box(suffix));
            black_box(s);
        });
    });
}

// ── 基准测试：小字符串 vs 大字符串 ──────────────────────────────────────────

fn bench_small_string_allocation(c: &mut Criterion) {
    let data = "small";

    c.bench_function("small_string_allocation", |b| {
        b.iter(|| {
            let _s: String = black_box(data).to_string();
            black_box(_s);
        });
    });
}

fn bench_large_string_allocation(c: &mut Criterion) {
    let data = "This is a very long string that will definitely not fit in the small string optimization buffer and will require heap allocation for storing its content";

    c.bench_function("large_string_allocation", |b| {
        b.iter(|| {
            let _s: String = black_box(data).to_string();
            black_box(_s);
        });
    });
}

// ── 基准测试组 ───────────────────────────────────────────────────────────────

criterion_group!(
    basic_allocation,
    bench_string_to_string,
    bench_string_borrow,
    bench_string_clone,
    bench_string_arc_clone,
);

criterion_group!(
    vec_allocation,
    bench_vec_string_allocation,
    bench_vec_str_borrow,
);

criterion_group!(
    string_concat,
    bench_format_macro,
    bench_string_concat,
);

criterion_group!(
    size_comparison,
    bench_small_string_allocation,
    bench_large_string_allocation,
);

criterion_main!(basic_allocation, vec_allocation, string_concat, size_comparison);
