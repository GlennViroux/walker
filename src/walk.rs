use crate::utils::{Histogram, MemorySize};
use crossbeam_channel::{Receiver, Sender, unbounded};
use std::collections::VecDeque;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

const PROGRESS_THRESHOLD: usize = 10_000;

fn process_one(current: PathBuf, histogram: &mut Histogram, mut enqueue: impl FnMut(PathBuf)) {
    if current.is_file() {
        match current.metadata() {
            Ok(m) => histogram.record(MemorySize::from_bytes(m.len())),
            Err(err) => eprintln!("unable to get size: {:?}", err),
        }
    } else {
        let read_dir = std::fs::read_dir(&current);
        match read_dir {
            Ok(read_dir) => {
                for dentry in read_dir {
                    match dentry {
                        Ok(dentry) => enqueue(dentry.path()),
                        Err(err) => eprintln!(
                            "Unable to read directory: {}. Error: {}",
                            &current.to_str().unwrap_or("unknown"),
                            err
                        ),
                    }
                }
            }
            Err(err) => eprintln!(
                "Unable to read directory: {}. Error: {}",
                &current.to_str().unwrap_or("unknown"),
                err
            ),
        }
    }
}

#[derive(Clone)]
struct WorkerConfig {
    tx: Sender<Option<PathBuf>>,
    rx: Arc<Receiver<Option<PathBuf>>>,
    pending: Arc<AtomicUsize>,
    total_processed: Arc<AtomicUsize>,
    bucket_count: u16,
    initial_bucket_size: Option<MemorySize>,
    num_threads: u8,
}

fn run_worker(_: u8, config: WorkerConfig) -> Histogram {
    let mut result: Histogram =
        Histogram::new(config.bucket_count as usize, config.initial_bucket_size);
    loop {
        let Ok(Some(current)) = config.rx.recv() else {
            break;
        };

        if current.is_file() {
            config.total_processed.fetch_add(1, Ordering::SeqCst);
            if config.total_processed.load(Ordering::SeqCst) % PROGRESS_THRESHOLD == 0 {
                print!(
                    "\rProcessed {} files",
                    config.total_processed.load(Ordering::SeqCst)
                );
                std::io::stdout().lock().flush().unwrap();
            }
        }

        process_one(current, &mut result, |path| {
            config.pending.fetch_add(1, Ordering::SeqCst);
            config.tx.send(Some(path)).unwrap();
        });
        if config.pending.fetch_sub(1, Ordering::SeqCst) == 1 {
            for _ in 1..=config.num_threads {
                config.tx.send(None).unwrap();
            }
        }
    }
    result
}

fn walk_parallel(
    root: &Path,
    num_threads: u8,
    num_buckets: u16,
    initial_bucket: Option<MemorySize>,
) -> Result<(), String> {
    println!("Running with {} threads", num_threads);
    let (tx, rx) = unbounded::<Option<PathBuf>>();
    let rx = Arc::new(rx);
    let pending = Arc::new(AtomicUsize::new(1));
    let total_processed = Arc::new(AtomicUsize::new(0));
    tx.send(Some(root.to_path_buf())).unwrap();

    let config = WorkerConfig {
        tx,
        rx,
        pending,
        total_processed,
        bucket_count: num_buckets,
        initial_bucket_size: initial_bucket,
        num_threads,
    };

    let handles: Vec<_> = (1..=num_threads)
        .map(|i| {
            let config = config.clone();
            std::thread::spawn(move || run_worker(i, config))
        })
        .collect();

    let result = handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .reduce(|mut a, b| {
            a.merge(b).expect("Unable to merge histograms together");
            a
        })
        .unwrap();

    print!("\n");
    println!("{}", result);
    Ok(())
}

fn walk(root: &Path, num_buckets: u16, initial_bucket: Option<MemorySize>) -> Result<(), String> {
    println!("Running single-threaded");
    if !root.exists() {
        eprintln!("Provided path: '{:?}' does not exist", root);
        std::process::exit(1);
    }

    let mut stdout = std::io::stdout().lock();
    let mut total_files: usize = 0;
    let mut queue: VecDeque<PathBuf> = VecDeque::new();
    queue.push_back(root.to_path_buf());

    let mut histogram = Histogram::new(num_buckets.into(), initial_bucket);

    while let Some(current) = queue.pop_front() {
        total_files += 1;
        if total_files % PROGRESS_THRESHOLD == 0 {
            print!("\rProcessed {} files", total_files);
            stdout.flush().unwrap();
        }

        let another = current.clone();
        let file_size = MemorySize::from_bytes(another.metadata().unwrap().len());
        histogram.record(file_size);

        process_one(current, &mut histogram, |dentry| queue.push_back(dentry));
    }

    print!("\n");
    println!("{}", histogram);
    Ok(())
}

pub fn run(
    root: &Path,
    num_threads: u8,
    num_buckets: u16,
    initial_bucket: Option<MemorySize>,
) -> Result<(), String> {
    match num_threads {
        0 => Err(String::from("At least one thread should be used")),
        1 => walk(root, num_buckets, initial_bucket),
        _ => walk_parallel(root, num_threads, num_buckets, initial_bucket),
    }
}
