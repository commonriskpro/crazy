//! Storage performance benchmarks: CAS, GraphStore, GC, compaction, CBOR codec.
//!
//! # Running
//!
//! Full benchmark suite (all groups):
//! ```text
//! cargo bench -p ail-storage
//! ```
//!
//! One group only (filter by name):
//! ```text
//! cargo bench -p ail-storage -- cas_put
//! cargo bench -p ail-storage -- snapshot_list
//! ```
//!
//! Compile without running (CI-safe):
//! ```text
//! cargo bench -p ail-storage --no-run
//! ```
//!
//! HTML reports land in `target/criterion/` when the `html_reports` feature is
//! enabled. The default feature set omits it to keep dependency surface small.

use std::hint::black_box;

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

use ail_storage::RetentionPolicy;
use ail_storage::backends::memory::MemoryObjectStore;
use ail_storage::codec::{CborCodec, ContentCodec};
use ail_storage::graph::{GraphStore, ObjectBackedGraphStore, SnapshotEnvelope};
use ail_storage::object::{ObjectId, ObjectStore, RawObject};
use ail_storage::{compact_snapshots, gc_unreferenced};

// ── runtime helpers ───────────────────────────────────────────────────────

fn new_rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("bench tokio runtime")
}

// ── fixture helpers ───────────────────────────────────────────────────────

/// Deterministic payload of `size` bytes (prime-modulo cycle, not all-zeros).
fn make_payload(size: usize) -> Vec<u8> {
    (0..size).map(|i| (i % 251) as u8).collect()
}

/// Deterministic `SnapshotEnvelope` derived from `seed`.
///
/// Uses `seed` for the envelope id and `seed ^ 0xdead_beef` for the root hash,
/// so every distinct seed produces a unique (id, graph_root_hash) pair.
fn make_snapshot(seed: u64) -> SnapshotEnvelope {
    let id = ObjectId::from_bytes(&seed.to_le_bytes());
    let root = ObjectId::from_bytes(&(seed ^ 0xdead_beef_dead_beef_u64).to_le_bytes());
    SnapshotEnvelope {
        id,
        graph_root_hash: root,
        parent_id: None,
        applied_change_id: None,
        created_at: seed.saturating_mul(1_000),
        verification_report_hash: None,
        audit_record_ids: Vec::new(),
        migration_metadata_ids: Vec::new(),
    }
}

/// Build a pre-populated `ObjectBackedGraphStore` with `n` unique snapshots.
fn populated_store(
    rt: &tokio::runtime::Runtime,
    n: usize,
) -> ObjectBackedGraphStore<MemoryObjectStore> {
    let store = ObjectBackedGraphStore::new(MemoryObjectStore::new());
    rt.block_on(async {
        for i in 0..n {
            store
                .save_snapshot(&make_snapshot(i as u64))
                .await
                .expect("bench setup: save_snapshot");
        }
    });
    store
}

// ── BLAKE3 / ObjectId hash benchmarks ────────────────────────────────────

fn bench_object_id_hash(c: &mut Criterion) {
    let mut g = c.benchmark_group("object_id_hash");
    for &size in &[64_usize, 4_096, 65_536] {
        let p = make_payload(size);
        g.throughput(Throughput::Bytes(size as u64));
        g.bench_with_input(BenchmarkId::from_parameter(size), &p, |b, p| {
            b.iter(|| black_box(ObjectId::from_bytes(p)));
        });
    }
    g.finish();
}

// ── CAS object-store benchmarks ───────────────────────────────────────────

fn bench_cas_put(c: &mut Criterion) {
    let rt = new_rt();
    let mut g = c.benchmark_group("cas_put");
    for &size in &[64_usize, 4_096, 65_536, 524_288] {
        let p = make_payload(size);
        g.throughput(Throughput::Bytes(size as u64));
        g.bench_with_input(BenchmarkId::from_parameter(size), &p, |b, p| {
            b.iter_batched(
                MemoryObjectStore::new,
                |store| {
                    rt.block_on(async {
                        black_box(store.put(RawObject(p.clone())).await.expect("put"))
                    })
                },
                BatchSize::SmallInput,
            );
        });
    }
    g.finish();
}

/// Idempotent put: store already contains the object — measures the fast path.
fn bench_cas_put_idempotent(c: &mut Criterion) {
    let rt = new_rt();
    let p = make_payload(4_096);
    let store = MemoryObjectStore::new();
    rt.block_on(async {
        store
            .put(RawObject(p.clone()))
            .await
            .expect("pre-warm put");
    });
    c.bench_function("cas_put_idempotent_4k", |b| {
        b.iter(|| {
            rt.block_on(async {
                black_box(store.put(RawObject(p.clone())).await.expect("idempotent put"))
            })
        });
    });
}

fn bench_cas_get(c: &mut Criterion) {
    let rt = new_rt();
    let mut g = c.benchmark_group("cas_get");
    for &size in &[64_usize, 4_096, 65_536] {
        let p = make_payload(size);
        let store = MemoryObjectStore::new();
        let id =
            rt.block_on(async { store.put(RawObject(p.clone())).await.expect("pre-warm put") });
        g.throughput(Throughput::Bytes(size as u64));
        g.bench_with_input(BenchmarkId::from_parameter(size), &id, |b, id| {
            b.iter(|| {
                rt.block_on(async { black_box(store.get(id).await.expect("get")) })
            });
        });
    }
    g.finish();
}

// ── CBOR codec benchmarks ─────────────────────────────────────────────────

fn bench_codec(c: &mut Criterion) {
    let codec = CborCodec;
    let snap = make_snapshot(42);
    let encoded = codec.encode(&snap).expect("encode");

    c.bench_function("cbor_encode_snapshot", |b| {
        b.iter(|| black_box(codec.encode(&snap).expect("encode")));
    });
    c.bench_function("cbor_decode_snapshot", |b| {
        b.iter(|| {
            let decoded: SnapshotEnvelope = codec.decode(&encoded).expect("decode");
            black_box(decoded)
        });
    });
    c.bench_function("cbor_roundtrip_snapshot", |b| {
        b.iter(|| {
            let bytes = codec.encode(&snap).expect("encode");
            let decoded: SnapshotEnvelope = codec.decode(&bytes).expect("decode");
            black_box(decoded)
        });
    });
}

// ── GraphStore scale benchmarks ───────────────────────────────────────────

fn bench_snapshot_save(c: &mut Criterion) {
    let rt = new_rt();
    let mut g = c.benchmark_group("snapshot_save");
    for &n in &[10_usize, 100, 500] {
        g.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched(
                || ObjectBackedGraphStore::new(MemoryObjectStore::new()),
                |store| {
                    rt.block_on(async {
                        for i in 0..n {
                            store
                                .save_snapshot(&make_snapshot(i as u64))
                                .await
                                .expect("save");
                        }
                    });
                },
                BatchSize::SmallInput,
            );
        });
    }
    g.finish();
}

fn bench_snapshot_list(c: &mut Criterion) {
    let rt = new_rt();
    let mut g = c.benchmark_group("snapshot_list");
    for &n in &[100_usize, 500, 1_000] {
        let store = populated_store(&rt, n);
        g.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| {
                rt.block_on(async {
                    black_box(store.list_snapshots().await.expect("list"))
                });
            });
        });
    }
    g.finish();
}

fn bench_snapshot_load(c: &mut Criterion) {
    let rt = new_rt();
    let snap = make_snapshot(999);
    let store = ObjectBackedGraphStore::new(MemoryObjectStore::new());
    rt.block_on(async {
        store.save_snapshot(&snap).await.expect("save");
    });
    c.bench_function("snapshot_load_by_id", |b| {
        b.iter(|| {
            rt.block_on(async {
                black_box(store.load_snapshot(&snap.id).await.expect("load"))
            });
        });
    });
}

// ── GC / compaction benchmarks ────────────────────────────────────────────

fn bench_gc(c: &mut Criterion) {
    let rt = new_rt();
    // Delete-all policy: measures raw GC throughput at scale.
    let policy = RetentionPolicy {
        max_age_days: None,
        keep_releases: false,
        keep_tagged: false,
    };
    let mut g = c.benchmark_group("gc_unreferenced");
    for &n in &[100_usize, 500] {
        g.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched(
                || populated_store(&rt, n),
                |store| {
                    rt.block_on(async {
                        black_box(
                            gc_unreferenced(&store, &policy, u64::MAX)
                                .await
                                .expect("gc"),
                        )
                    });
                },
                BatchSize::SmallInput,
            );
        });
    }
    g.finish();
}

fn bench_compact(c: &mut Criterion) {
    let rt = new_rt();
    const N: usize = 50;
    c.bench_function("compact_50_snapshots", |b| {
        b.iter_batched(
            || populated_store(&rt, N),
            |store| {
                rt.block_on(async {
                    black_box(compact_snapshots(&store, 0, N - 1).await.expect("compact"));
                });
            },
            BatchSize::SmallInput,
        );
    });
}

// ── registry ─────────────────────────────────────────────────────────────

criterion_group!(
    benches,
    bench_object_id_hash,
    bench_cas_put,
    bench_cas_put_idempotent,
    bench_cas_get,
    bench_codec,
    bench_snapshot_save,
    bench_snapshot_list,
    bench_snapshot_load,
    bench_gc,
    bench_compact,
);
criterion_main!(benches);
