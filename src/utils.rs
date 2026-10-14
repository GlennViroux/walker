use std::fmt;
use std::fmt::Formatter;
use std::iter::Sum;
use std::ops::{Add, AddAssign};

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Default)]
pub struct MemorySize(u64);

impl MemorySize {
    pub fn from_bytes(bytes: u64) -> Self {
        MemorySize(bytes)
    }

    pub fn from_kb(kbytes: u64) -> Self {
        Self::from_bytes(kbytes * 1u64 << 10)
    }

    pub fn from_mb(mbytes: u64) -> Self {
        Self::from_kb(mbytes * 1u64 << 10)
    }

    pub fn bytes(&self) -> u64 {
        self.0
    }

    fn to_kb(&self) -> f64 {
        self.0 as f64 / (1u64 << 10) as f64
    }
    fn to_mb(&self) -> f64 {
        self.0 as f64 / (1u64 << 20) as f64
    }
    fn to_gb(&self) -> f64 {
        self.0 as f64 / (1u64 << 30) as f64
    }
    fn to_tb(&self) -> f64 {
        self.0 as f64 / (1u64 << 40) as f64
    }
    fn to_pb(&self) -> f64 {
        self.0 as f64 / (1u64 << 50) as f64
    }
}

impl From<u64> for MemorySize {
    fn from(value: u64) -> Self {
        MemorySize::from_bytes(value)
    }
}

impl Sum for MemorySize {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(MemorySize::default(), |a, b| a + b)
    }
}

impl Add for MemorySize {
    type Output = MemorySize;
    fn add(self, rhs: Self) -> Self::Output {
        MemorySize(self.0 + rhs.0)
    }
}

impl Add<u64> for MemorySize {
    type Output = MemorySize;
    fn add(self, rhs: u64) -> Self::Output {
        MemorySize(self.0 + rhs)
    }
}

impl AddAssign for MemorySize {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl AddAssign<u64> for MemorySize {
    fn add_assign(&mut self, rhs: u64) {
        self.0 += rhs;
    }
}

impl fmt::Display for MemorySize {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self.0 {
            0..=1_023 => write!(f, "{:.2}B", self.bytes()),
            1_024..=1_048_575 => write!(f, "{:.2} kB", self.to_kb()),
            1_048_576..=1_073_741_823 => write!(f, "{:.2} MB", self.to_mb()),
            1_073_741_824..=1_099_511_627_775 => write!(f, "{:.2} GB", self.to_gb()),
            1_099_511_627_776..=1_125_899_906_842_623 => write!(f, "{:.2} TB", self.to_tb()),
            _ => write!(f, "{} PB", self.to_pb()),
        }
    }
}

struct BucketBoundaries {
    upper_bounds: Vec<MemorySize>,
}

impl BucketBoundaries {
    fn new(count: usize, initial_bucket: Option<MemorySize>) -> Self {
        let mut upper_bounds: Vec<MemorySize> = Vec::with_capacity(count);
        let mut bound = initial_bucket.unwrap_or(MemorySize::from_kb(16));
        for _ in 0..count.saturating_sub(1) {
            upper_bounds.push(bound);
            bound.0 = bound.0.saturating_mul(2);
        }
        BucketBoundaries { upper_bounds }
    }
    fn bucket_index(&self, file: &MemorySize) -> usize {
        self.upper_bounds.binary_search(file).unwrap_or_else(|i| i)
    }
}

impl PartialEq for BucketBoundaries {
    fn eq(&self, other: &Self) -> bool {
        self.upper_bounds == other.upper_bounds
    }
}

pub struct Histogram {
    boundaries: BucketBoundaries,
    count: u64,
    mean: f64,
    m2: f64, // sum of squares of differences of current mean
    bucket_counts: Vec<u64>,
    bucket_totals: Vec<MemorySize>,
}

impl Histogram {
    pub fn new(count: usize, initial_bucket: Option<MemorySize>) -> Self {
        Histogram {
            boundaries: BucketBoundaries::new(count, initial_bucket),
            count: 0,
            mean: 0.0,
            m2: 0.0,
            bucket_counts: vec![0; count],
            bucket_totals: vec![MemorySize::from_bytes(0); count],
        }
    }

    pub fn record(&mut self, file: MemorySize) {
        let bucket_index = self.boundaries.bucket_index(&file);
        self.bucket_counts[bucket_index] += 1;
        self.bucket_totals[bucket_index] += file;
        self.count += 1;

        let value = file.0 as f64;
        let delta = value - self.mean;
        self.mean += delta / self.count as f64;
        let delta2 = value - self.mean;
        self.m2 += delta * delta2;
    }

    pub fn total_size(&self) -> MemorySize {
        self.bucket_totals.iter().copied().sum::<MemorySize>()
    }

    pub fn mean(&self) -> MemorySize {
        MemorySize::from_bytes(self.mean as u64)
    }

    pub fn variance(&self) -> MemorySize {
        let result = if self.count < 2 {
            0.0
        } else {
            self.m2 / self.count as f64
        };
        MemorySize::from_bytes(result as u64)
    }

    pub fn std_dev(&self) -> MemorySize {
        MemorySize::from_bytes(self.variance().0.isqrt())
    }

    pub fn merge(&mut self, other: Histogram) -> Result<(), &'static str> {
        if self.boundaries != other.boundaries {
            return Err("Histograms with different bucket boundaries cannot be merged");
        }

        if other.count == 0 {
        } else if self.count == 0 {
            self.count = other.count;
            self.mean = other.mean;
            self.m2 = other.m2;
            self.bucket_counts = other.bucket_counts;
            self.bucket_totals = other.bucket_totals;
            return Ok(());
        } else {
            let count = self.count + other.count;
            let delta = other.mean - self.mean;
            let mean = self.mean + delta * other.count as f64 / count as f64;
            let m2 = self.m2
                + other.m2
                + delta * delta * self.count as f64 * other.count as f64 / count as f64;

            self.count = count;
            self.mean = mean;
            self.m2 = m2;
        }

        for (a, b) in self.bucket_totals.iter_mut().zip(&other.bucket_totals) {
            *a += *b;
        }

        for (a, b) in self.bucket_counts.iter_mut().zip(&other.bucket_counts) {
            *a += *b;
        }

        Ok(())
    }
}

impl fmt::Display for Histogram {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "Walker - {} files - total {} - mean {} - std dev {}",
            self.count,
            self.total_size(),
            self.mean(),
            self.std_dev()
        )?;

        let max_count = *self.bucket_counts.iter().max().unwrap_or(&1);
        let bin_width = 100;

        let last = self.bucket_counts.len() - 1;
        for idx in 0..=last {
            let lower = if idx == 0 {
                "0B".to_string()
            } else {
                format!("{}", self.boundaries.upper_bounds[idx - 1])
            };
            let upper = if idx == last {
                "∞".to_string()
            } else {
                format!("{}", self.boundaries.upper_bounds[idx])
            };
            let count = self.bucket_counts[idx];

            let bar_len = if max_count == 0 || count == 0 {
                0
            } else {
                count * bin_width / max_count
            };
            writeln!(
                f,
                "{:<10} -> {:<10} {:<10} | {}",
                lower,
                upper,
                format!("({})", count),
                "█".repeat(bar_len as usize)
            )?;
        }

        Ok(())
    }
}
