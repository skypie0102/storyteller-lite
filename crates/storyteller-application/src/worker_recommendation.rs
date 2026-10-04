//! A lightweight CPU/RAM starting recommendation, not a throughput benchmark.
use storyteller_core::MAX_TRANSCRIPTION_WORKERS;

const MEMORY_PER_WORKER_MIB: u64 = 512;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerRecommendation {
    pub logical_cpu_threads: Option<usize>,
    pub available_memory_mib: Option<u64>,
    pub recommended_workers: usize,
}

impl Default for WorkerRecommendation {
    fn default() -> Self {
        Self::from_hardware(None, None)
    }
}

impl WorkerRecommendation {
    pub fn from_hardware(
        logical_cpu_threads: Option<usize>,
        available_memory_mib: Option<u64>,
    ) -> Self {
        let logical_cpu_threads = logical_cpu_threads.filter(|threads| *threads > 0);
        // Use about half the available logical threads, leaving capacity for
        // conversion and the desktop. This does not assume SMT or a CPU model.
        let cpu_limit = logical_cpu_threads.map_or(1, |threads| (threads / 2).max(1));
        // Reserve part of currently available RAM. The allowance covers engine
        // working memory and conversion, not just the 16.9 MB model file.
        let memory_limit = available_memory_mib.map_or(1, |available| {
            let reserve = (available / 4).clamp(512, 2_048);
            (available.saturating_sub(reserve) / MEMORY_PER_WORKER_MIB)
                .clamp(1, MAX_TRANSCRIPTION_WORKERS as u64) as usize
        });
        Self {
            logical_cpu_threads,
            available_memory_mib,
            recommended_workers: cpu_limit
                .min(memory_limit)
                .clamp(1, MAX_TRANSCRIPTION_WORKERS),
        }
    }

    /// Zero is the UI's Automatic selection; jobs always save a concrete count.
    pub fn resolve_selection(&self, selection: i32) -> Result<usize, String> {
        if selection == 0 {
            return Ok(self.recommended_workers.clamp(1, MAX_TRANSCRIPTION_WORKERS));
        }
        let count = usize::try_from(selection).unwrap_or(0);
        if !(1..=MAX_TRANSCRIPTION_WORKERS).contains(&count) {
            return Err(format!(
                "CPU workers must be Automatic or 1–{MAX_TRANSCRIPTION_WORKERS}."
            ));
        }
        Ok(count)
    }

    pub fn description(&self) -> String {
        let cpu = self.logical_cpu_threads.map_or_else(
            || "CPU availability unknown".into(),
            |threads| format!("{threads} logical CPU threads available"),
        );
        let memory = self.available_memory_mib.map_or_else(
            || "RAM availability unknown".into(),
            |mib| format!("{:.1} GiB RAM available", mib as f64 / 1_024.0),
        );
        format!(
            "{cpu} · {memory}. Starting recommendation: {} worker{}.",
            self.recommended_workers,
            if self.recommended_workers == 1 {
                ""
            } else {
                "s"
            }
        )
    }
}

pub(crate) fn detect_worker_recommendation() -> WorkerRecommendation {
    let cpu = std::thread::available_parallelism()
        .ok()
        .map(|count| count.get());
    WorkerRecommendation::from_hardware(cpu, available_memory_mib())
}

#[cfg(windows)]
fn available_memory_mib() -> Option<u64> {
    // MEMORYSTATUSEX has a fixed Win32 layout; no additional runtime or crate.
    #[repr(C)]
    #[derive(Default)]
    struct MemoryStatus {
        length: u32,
        memory_load: u32,
        total_phys: u64,
        available_phys: u64,
        total_page_file: u64,
        available_page_file: u64,
        total_virtual: u64,
        available_virtual: u64,
        available_extended_virtual: u64,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GlobalMemoryStatusEx(status: *mut MemoryStatus) -> i32;
    }
    let mut status = MemoryStatus {
        length: std::mem::size_of::<MemoryStatus>() as u32,
        ..Default::default()
    };
    // SAFETY: initialized writable storage has the documented C layout and
    // length, remains alive during this synchronous call and is not retained.
    (unsafe { GlobalMemoryStatusEx(&mut status) } != 0)
        .then_some(status.available_phys / (1_024 * 1_024))
}

#[cfg(target_os = "linux")]
fn available_memory_mib() -> Option<u64> {
    let contents = std::fs::read_to_string("/proc/meminfo").ok()?;
    parse_available_memory(&contents)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn available_memory_mib() -> Option<u64> {
    None
}

#[cfg(any(target_os = "linux", test))]
fn parse_available_memory(contents: &str) -> Option<u64> {
    let value = contents
        .lines()
        .find_map(|line| line.strip_prefix("MemAvailable:"))?;
    let mut fields = value.split_whitespace();
    let kib = fields.next()?.parse::<u64>().ok()?;
    if fields.next()? != "kB" || fields.next().is_some() {
        return None;
    }
    Some(kib / 1_024)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recommendation_scales_with_cpu_above_four_but_is_bounded() {
        for (threads, expected) in [(1, 1), (2, 1), (8, 4), (16, 8), (32, 16), (128, 16)] {
            let recommendation = WorkerRecommendation::from_hardware(Some(threads), Some(32_768));
            assert_eq!(recommendation.recommended_workers, expected);
        }
    }

    #[test]
    fn low_available_memory_reduces_recommendation_without_using_total_ram() {
        for (available, expected) in [(0, 1), (512, 1), (1_536, 2), (3_072, 4), (8_192, 12)] {
            let recommendation = WorkerRecommendation::from_hardware(Some(64), Some(available));
            assert_eq!(recommendation.recommended_workers, expected);
        }
    }

    #[test]
    fn missing_cpu_or_memory_falls_back_to_one() {
        for (cpu, memory) in [
            (None, None),
            (None, Some(32_768)),
            (Some(32), None),
            (Some(0), Some(32_768)),
        ] {
            assert_eq!(
                WorkerRecommendation::from_hardware(cpu, memory).recommended_workers,
                1
            );
        }
    }

    #[test]
    fn automatic_resolves_concretely_and_manual_override_survives_recommendation() {
        let recommendation = WorkerRecommendation::from_hardware(Some(16), Some(8_192));
        assert_eq!(recommendation.resolve_selection(0).unwrap(), 8);
        assert_eq!(recommendation.resolve_selection(1).unwrap(), 1);
        assert_eq!(recommendation.resolve_selection(16).unwrap(), 16);
        assert!(recommendation.resolve_selection(-1).is_err());
        assert!(recommendation.resolve_selection(17).is_err());
    }

    #[test]
    fn description_distinguishes_known_hardware_and_missing_information() {
        let known = WorkerRecommendation::from_hardware(Some(16), Some(8_192)).description();
        assert!(known.contains("16 logical CPU threads"));
        assert!(known.contains("8.0 GiB RAM available"));
        assert!(known.contains("8 workers"));
        let fallback = WorkerRecommendation::default().description();
        assert!(fallback.contains("RAM availability unknown"));
        assert!(fallback.contains("1 worker."));
    }

    #[test]
    fn linux_memory_parser_requires_available_memory_and_correct_units() {
        assert_eq!(
            parse_available_memory("MemTotal: 999999 kB\nMemAvailable: 8388608 kB\n"),
            Some(8_192)
        );
        for value in [
            "MemTotal: 8388608 kB",
            "MemAvailable: -1 kB",
            "MemAvailable: 128 MB",
            "MemAvailable: 128 kB extra",
        ] {
            assert_eq!(parse_available_memory(value), None);
        }
    }

    #[test]
    fn native_probe_returns_a_bounded_recommendation() {
        let recommendation = detect_worker_recommendation();
        assert!((1..=MAX_TRANSCRIPTION_WORKERS).contains(&recommendation.recommended_workers));
        #[cfg(windows)]
        assert!(
            recommendation.available_memory_mib.is_some(),
            "Win32 memory probe failed"
        );
    }
}
