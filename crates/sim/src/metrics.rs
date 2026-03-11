//! Bandwidth and propagation metrics collected from simulation runs.

use core::fmt::{self, Write as _};
use core::time::Duration;

use tracing::info;

/// Metrics collected from a single block propagation simulation.
#[derive(Clone, Debug)]
pub struct SimMetrics {
    /// Strategy name.
    pub strategy: String,
    /// Number of nodes.
    pub num_nodes: usize,
    /// Mesh degree used.
    pub mesh_degree: usize,
    /// Block size in bytes.
    pub block_size: usize,

    // -- Bandwidth --
    /// Total bytes sent across all links.
    pub total_bytes_sent: u64,
    /// Useful bytes = `block_size * nodes_decoded`.
    pub useful_bytes: u64,
    /// Redundancy ratio = `total_bytes_sent / useful_bytes`.
    pub redundancy_ratio: f64,
    /// Per-node upload bytes.
    pub per_node_upload: Vec<u64>,
    /// Per-node download bytes.
    pub per_node_download: Vec<u64>,
    /// Mean per-node upload.
    pub mean_upload: f64,
    /// Max per-node upload.
    pub max_upload: u64,

    // -- Propagation --
    /// Time at which each node decoded (`None` if not decoded).
    pub decode_times: Vec<Option<Duration>>,
    /// Number of nodes that successfully decoded.
    pub nodes_decoded: usize,
    /// Time until last node decoded (worst-case propagation).
    pub propagation_time: Duration,
    /// Time until 50th percentile decoded.
    pub p50_time: Duration,
    /// Time until 99th percentile decoded.
    pub p99_time: Duration,

    // -- Efficiency --
    /// Total packets sent.
    pub total_packets: u64,
    /// Useful packets (carried new information).
    pub useful_packets: u64,
    /// Redundant packets (duplicate / linearly dependent).
    pub redundant_packets: u64,
}

impl fmt::Display for SimMetrics {
    #[allow(clippy::cast_precision_loss)]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{} (D={})", self.strategy, self.mesh_degree)?;
        writeln!(
            f,
            "  Nodes decoded:    {}/{}",
            self.nodes_decoded, self.num_nodes
        )?;
        writeln!(f, "  Block size:       {} bytes", self.block_size)?;
        writeln!(
            f,
            "  Total BW:         {:.2} MB",
            self.total_bytes_sent as f64 / 1_000_000.0
        )?;
        writeln!(
            f,
            "  Useful BW:        {:.2} MB",
            self.useful_bytes as f64 / 1_000_000.0
        )?;
        writeln!(f, "  Redundancy:       {:.2}x", self.redundancy_ratio)?;
        writeln!(
            f,
            "  Propagation:      {:.1} ms",
            duration_ms(self.propagation_time)
        )?;
        writeln!(
            f,
            "  p50:              {:.1} ms",
            duration_ms(self.p50_time)
        )?;
        writeln!(
            f,
            "  p99:              {:.1} ms",
            duration_ms(self.p99_time)
        )?;
        writeln!(
            f,
            "  Packets:          {} total, {} useful, {} redundant",
            self.total_packets, self.useful_packets, self.redundant_packets
        )?;
        writeln!(
            f,
            "  Mean upload:      {:.2} MB",
            self.mean_upload / 1_000_000.0
        )?;
        write!(
            f,
            "  Max upload:       {:.2} MB",
            self.max_upload as f64 / 1_000_000.0
        )
    }
}

/// Cumulative metrics across multiple blocks.
#[derive(Clone, Debug)]
pub struct CumulativeMetrics {
    /// Strategy name.
    pub name: String,
    /// Per-block results.
    pub blocks: Vec<SimMetrics>,
}

impl CumulativeMetrics {
    /// Create a new accumulator.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            blocks: Vec::new(),
        }
    }

    /// Record a single block's metrics.
    pub fn accumulate(&mut self, metrics: SimMetrics) {
        self.blocks.push(metrics);
    }

    /// Total bytes sent across all blocks.
    pub fn total_bytes(&self) -> u64 {
        self.blocks.iter().map(|m| m.total_bytes_sent).sum()
    }

    /// Mean total bytes per block.
    #[allow(clippy::cast_precision_loss)]
    pub fn mean_total_bytes(&self) -> f64 {
        if self.blocks.is_empty() {
            return 0.0;
        }
        self.total_bytes() as f64 / self.blocks.len() as f64
    }

    /// Mean useful bytes per block.
    #[allow(clippy::cast_precision_loss)]
    pub fn mean_useful_bytes(&self) -> f64 {
        if self.blocks.is_empty() {
            return 0.0;
        }
        let sum: u64 = self.blocks.iter().map(|m| m.useful_bytes).sum();
        sum as f64 / self.blocks.len() as f64
    }

    /// Mean redundancy ratio.
    pub fn mean_redundancy(&self) -> f64 {
        let useful = self.mean_useful_bytes();
        if useful == 0.0 {
            return 0.0;
        }
        self.mean_total_bytes() / useful
    }

    /// Mean propagation time.
    ///
    /// # Panics
    ///
    /// Panics if the block count exceeds `u32::MAX`.
    pub fn mean_propagation(&self) -> Duration {
        if self.blocks.is_empty() {
            return Duration::ZERO;
        }
        let sum: Duration = self.blocks.iter().map(|m| m.propagation_time).sum();
        let count = u32::try_from(self.blocks.len()).expect("block count exceeds u32");
        sum / count
    }

    /// Mean p99 propagation time.
    ///
    /// # Panics
    ///
    /// Panics if the block count exceeds `u32::MAX`.
    pub fn mean_p99(&self) -> Duration {
        if self.blocks.is_empty() {
            return Duration::ZERO;
        }
        let sum: Duration = self.blocks.iter().map(|m| m.p99_time).sum();
        let count = u32::try_from(self.blocks.len()).expect("block count exceeds u32");
        sum / count
    }

    /// Mean nodes decoded per block.
    #[allow(clippy::cast_precision_loss)]
    pub fn mean_nodes_decoded(&self) -> f64 {
        if self.blocks.is_empty() {
            return 0.0;
        }
        let sum: usize = self.blocks.iter().map(|m| m.nodes_decoded).sum();
        sum as f64 / self.blocks.len() as f64
    }
}

/// Print a side-by-side comparison table of cumulative metrics.
pub fn compare(results: &[&CumulativeMetrics]) {
    if results.is_empty() || results[0].blocks.is_empty() {
        return;
    }

    let first = &results[0].blocks[0];
    let mut t = String::new();

    writeln!(
        t,
        "\nStrategy Comparison ({} nodes, {} blocks of ~{} bytes)",
        first.num_nodes,
        results[0].blocks.len(),
        first.block_size,
    )
    .unwrap();
    writeln!(t, "{}", "=".repeat(70)).unwrap();

    // Header.
    write!(t, "{:<25}", "").unwrap();
    for r in results {
        let label = if r.blocks.is_empty() {
            r.name.clone()
        } else {
            format!("{} (D={})", r.name, r.blocks[0].mesh_degree)
        };
        write!(t, "{label:<20}").unwrap();
    }
    writeln!(t).unwrap();
    writeln!(t, "{}", "-".repeat(70)).unwrap();

    // Mean total BW.
    write!(t, "{:<25}", "Avg total BW/block").unwrap();
    for r in results {
        write!(t, "{:<20}", format_bytes(r.mean_total_bytes())).unwrap();
    }
    writeln!(t).unwrap();

    // Mean useful BW.
    write!(t, "{:<25}", "Avg useful BW/block").unwrap();
    for r in results {
        write!(t, "{:<20}", format_bytes(r.mean_useful_bytes())).unwrap();
    }
    writeln!(t).unwrap();

    // Redundancy.
    write!(t, "{:<25}", "Redundancy ratio").unwrap();
    for r in results {
        write!(t, "{:<20}", format!("{:.2}x", r.mean_redundancy())).unwrap();
    }
    writeln!(t).unwrap();

    // Propagation.
    write!(t, "{:<25}", "Avg propagation").unwrap();
    for r in results {
        write!(
            t,
            "{:<20}",
            format!("{:.1} ms", duration_ms(r.mean_propagation()))
        )
        .unwrap();
    }
    writeln!(t).unwrap();

    // p99.
    write!(t, "{:<25}", "Avg p99 propagation").unwrap();
    for r in results {
        write!(t, "{:<20}", format!("{:.1} ms", duration_ms(r.mean_p99()))).unwrap();
    }
    writeln!(t).unwrap();

    // Nodes decoded.
    write!(t, "{:<25}", "Avg nodes decoded").unwrap();
    for r in results {
        let total = if r.blocks.is_empty() {
            0
        } else {
            r.blocks[0].num_nodes
        };
        write!(
            t,
            "{:<20}",
            format!("{:.0}/{total}", r.mean_nodes_decoded())
        )
        .unwrap();
    }
    writeln!(t).unwrap();

    writeln!(t, "{}", "=".repeat(70)).unwrap();

    info!("{t}");
}

fn duration_ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn format_bytes(bytes: f64) -> String {
    if bytes >= 1_000_000_000.0 {
        format!("{:.1} GB", bytes / 1_000_000_000.0)
    } else if bytes >= 1_000_000.0 {
        format!("{:.1} MB", bytes / 1_000_000.0)
    } else if bytes >= 1_000.0 {
        format!("{:.1} KB", bytes / 1_000.0)
    } else {
        format!("{bytes:.0} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cumulative_metrics_basic() {
        let mut cm = CumulativeMetrics::new("test");
        assert_eq!(cm.total_bytes(), 0);
        assert_eq!(cm.mean_total_bytes(), 0.0);

        let m = SimMetrics {
            strategy: "test".into(),
            num_nodes: 10,
            mesh_degree: 4,
            block_size: 1000,
            total_bytes_sent: 40_000,
            useful_bytes: 10_000,
            redundancy_ratio: 4.0,
            per_node_upload: vec![4000; 10],
            per_node_download: vec![4000; 10],
            mean_upload: 4000.0,
            max_upload: 4000,
            decode_times: vec![Some(Duration::from_millis(100)); 10],
            nodes_decoded: 10,
            propagation_time: Duration::from_millis(100),
            p50_time: Duration::from_millis(80),
            p99_time: Duration::from_millis(95),
            total_packets: 40,
            useful_packets: 30,
            redundant_packets: 10,
        };

        cm.accumulate(m);
        assert_eq!(cm.total_bytes(), 40_000);
        assert!((cm.mean_redundancy() - 4.0).abs() < 0.01);
    }
}
