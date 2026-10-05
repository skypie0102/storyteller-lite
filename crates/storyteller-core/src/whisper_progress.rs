use crate::CommandStream;
use std::path::PathBuf;

/// Decoder progress for one known input, before its JSON result is validated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WhisperNativeProgress {
    pub input_index: usize,
    pub percent: u8,
}

/// Reads the pinned whisper.cpp CLI's stderr protocol. Transcript stdout is
/// never interpreted as diagnostics or progress. A skipped input earns no work.
#[derive(Debug)]
pub struct WhisperProgressTracker {
    markers: Vec<(String, String)>,
    active: Option<usize>,
    last_input: Option<usize>,
    percent: u8,
}

impl WhisperProgressTracker {
    pub fn new(inputs: &[(PathBuf, PathBuf)]) -> Self {
        Self {
            markers: inputs
                .iter()
                .map(|(audio, prefix)| {
                    (
                        format!("main: processing '{}' (", audio.display()),
                        format!("output_json: saving output to '{}.json'", prefix.display()),
                    )
                })
                .collect(),
            active: None,
            last_input: None,
            percent: 0,
        }
    }

    pub fn observe(&mut self, stream: CommandStream, line: &str) -> Option<WhisperNativeProgress> {
        if stream != CommandStream::Stderr {
            return None;
        }
        if line.starts_with("main: processing '") {
            // Unknown/repeated/out-of-order processing markers cannot lend
            // their following percentages to a different input.
            self.active = None;
            let index = self
                .markers
                .iter()
                .position(|(marker, _)| line.starts_with(marker))?;
            if self.last_input.is_some_and(|last| index <= last) {
                return None;
            }
            self.active = Some(index);
            self.last_input = Some(index);
            self.percent = 0;
            return Some(WhisperNativeProgress {
                input_index: index,
                percent: 0,
            });
        }
        let index = self.active?;
        let percent = if line == self.markers[index].1 {
            // This marker proves inference finished, not that the JSON is
            // complete or usable. The application still validates every file.
            self.active = None;
            100
        } else {
            let value = line
                .strip_prefix("whisper_print_progress_callback: progress = ")?
                .strip_suffix('%')?
                .trim();
            if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            value.parse::<u8>().ok().filter(|value| *value <= 100)?
        };
        if percent <= self.percent {
            return None;
        }
        self.percent = percent;
        Some(WhisperNativeProgress {
            input_index: index,
            percent,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracker() -> WhisperProgressTracker {
        WhisperProgressTracker::new(&[
            (
                "C:/O'Brien/first input.wav".into(),
                "C:/first result".into(),
            ),
            ("C:/second.wav".into(), "C:/second result".into()),
            ("C:/third.wav".into(), "C:/third result".into()),
        ])
    }

    #[test]
    fn tracks_native_file_resets_without_crediting_skipped_inputs() {
        let mut tracker = tracker();
        let lines = [
            "whisper_print_progress_callback: progress =  50%",
            "main: processing 'C:/O'Brien/first input.wav' (100 samples, 1.0 sec)",
            "whisper_print_progress_callback: progress =  50%",
            "output_json: saving output to 'C:/first result.json'",
            "error: failed to read audio file 'C:/second.wav'",
            "main: processing 'C:/third.wav' (100 samples, 1.0 sec)",
            "whisper_print_progress_callback: progress =  10%",
        ];
        let events = lines
            .iter()
            .filter_map(|line| tracker.observe(CommandStream::Stderr, line))
            .map(|event| (event.input_index, event.percent))
            .collect::<Vec<_>>();
        assert_eq!(events, vec![(0, 0), (0, 50), (0, 100), (2, 0), (2, 10)]);
    }

    #[test]
    fn ignores_narrator_stdout_and_malformed_or_regressing_percentages() {
        let mut tracker = tracker();
        let processing = "main: processing 'C:/second.wav' (100 samples, 1.0 sec)";
        assert!(tracker.observe(CommandStream::Stdout, processing).is_none());
        assert!(tracker
            .observe(CommandStream::Stderr, "progress = 50%")
            .is_none());
        tracker.observe(CommandStream::Stderr, processing).unwrap();
        assert_eq!(
            tracker
                .observe(
                    CommandStream::Stderr,
                    "whisper_print_progress_callback: progress =  50%"
                )
                .unwrap()
                .percent,
            50
        );
        for value in ["50", "20", "101", "-1", "+60", "NaN", "", "80% extra"] {
            assert!(tracker
                .observe(
                    CommandStream::Stderr,
                    &format!("whisper_print_progress_callback: progress = {value}%")
                )
                .is_none());
        }
        assert!(tracker
            .observe(
                CommandStream::Stdout,
                "output_json: saving output to 'C:/second result.json'"
            )
            .is_none());
        assert!(tracker
            .observe(
                CommandStream::Stderr,
                "output_json: saving output to 'C:/third result.json'"
            )
            .is_none());
    }

    #[test]
    fn unknown_or_backward_file_marker_invalidates_active_progress() {
        let mut tracker = tracker();
        for marker in [
            "main: processing 'C:/second.wav' (100 samples)",
            "main: processing 'C:/unexpected.wav' (100 samples)",
            "main: processing 'C:/O'Brien/first input.wav' (100 samples)",
        ] {
            tracker.observe(CommandStream::Stderr, marker);
        }
        assert!(tracker
            .observe(
                CommandStream::Stderr,
                "whisper_print_progress_callback: progress =  90%"
            )
            .is_none());
    }
}
