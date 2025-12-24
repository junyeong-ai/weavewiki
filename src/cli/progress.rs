//! Real-Time Progress Streaming
//!
//! Multi-channel progress reporting for CLI and TUI.
//! Based on deepwiki-open's real-time progress pattern.
//!
//! ## Features
//!
//! - Multiple progress channels (file, module, phase)
//! - ETA calculation
//! - Throughput monitoring
//! - Event-based updates

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use tokio::sync::broadcast;

/// Progress event types
#[derive(Debug, Clone)]
pub enum ProgressEvent {
    /// Phase started
    PhaseStarted {
        phase: u8,
        phase_name: String,
        total_items: usize,
    },
    /// Phase completed
    PhaseCompleted { phase: u8, duration_secs: u64 },
    /// Item progress (file, batch, etc.)
    ItemProgress {
        completed: usize,
        total: usize,
        current_item: String,
        throughput: f32, // items per second
    },
    /// Sub-task progress (within an item)
    SubProgress {
        parent_item: String,
        completed: usize,
        total: usize,
    },
    /// Status message
    Message {
        level: MessageLevel,
        message: String,
    },
    /// Estimated time remaining
    EtaUpdate { remaining_secs: u64 },
    /// Error occurred
    Error {
        item: String,
        error: String,
        recoverable: bool,
    },
    /// Pipeline finished
    Finished {
        success: bool,
        total_duration_secs: u64,
        summary: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageLevel {
    Debug,
    Info,
    Warning,
    Error,
}

/// Progress tracker state
#[derive(Debug, Clone)]
pub struct ProgressState {
    /// Current phase (1-7)
    pub phase: u8,
    /// Phase name
    pub phase_name: String,
    /// Items completed in current phase
    pub completed: usize,
    /// Total items in current phase
    pub total: usize,
    /// Current item being processed
    pub current_item: String,
    /// Overall progress (0.0-1.0)
    pub overall_progress: f32,
    /// Items processed per second
    pub throughput: f32,
    /// Estimated seconds remaining
    pub eta_secs: Option<u64>,
    /// Whether currently running
    pub is_running: bool,
    /// Total elapsed time
    pub elapsed_secs: u64,
}

impl Default for ProgressState {
    fn default() -> Self {
        Self {
            phase: 0,
            phase_name: String::new(),
            completed: 0,
            total: 0,
            current_item: String::new(),
            overall_progress: 0.0,
            throughput: 0.0,
            eta_secs: None,
            is_running: false,
            elapsed_secs: 0,
        }
    }
}

/// Real-time progress tracker
pub struct ProgressTracker {
    /// Current state
    state: Arc<RwLock<ProgressState>>,
    /// Event broadcast channel
    sender: broadcast::Sender<ProgressEvent>,
    /// Start time
    start_time: Arc<RwLock<Option<Instant>>>,
    /// Phase start times for accurate ETA
    phase_times: Arc<RwLock<Vec<(u8, Instant, u64)>>>,
    /// Whether tracking is active
    active: Arc<AtomicBool>,
    /// Total phases (for overall progress)
    total_phases: u8,
    /// Items processed counter (for throughput)
    items_processed: Arc<AtomicU64>,
}

impl ProgressTracker {
    /// Create a new progress tracker
    pub fn new(total_phases: u8) -> Self {
        let (sender, _) = broadcast::channel(256);

        Self {
            state: Arc::new(RwLock::new(ProgressState::default())),
            sender,
            start_time: Arc::new(RwLock::new(None)),
            phase_times: Arc::new(RwLock::new(Vec::new())),
            active: Arc::new(AtomicBool::new(false)),
            total_phases,
            items_processed: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Send an event through the broadcast channel.
    /// Silently discards if no receivers are listening (expected when UI is not connected).
    #[inline]
    fn emit(&self, event: ProgressEvent) {
        // Receivers may not exist if no UI is attached - this is normal operation
        let _ = self.sender.send(event);
    }

    /// Subscribe to progress events
    pub fn subscribe(&self) -> broadcast::Receiver<ProgressEvent> {
        self.sender.subscribe()
    }

    /// Get current state
    pub fn state(&self) -> ProgressState {
        self.state
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Start tracking
    pub fn start(&self) {
        self.active.store(true, Ordering::SeqCst);
        *self
            .start_time
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Instant::now());

        let mut state = self
            .state
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.is_running = true;
        state.phase = 0;
    }

    /// Stop tracking
    pub fn stop(&self) {
        self.active.store(false, Ordering::SeqCst);
        self.state
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_running = false;
    }

    /// Start a new phase
    pub fn start_phase(&self, phase: u8, phase_name: &str, total_items: usize) {
        let now = Instant::now();

        // Record phase start
        self.phase_times
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push((phase, now, total_items as u64));

        // Update state
        {
            let mut state = self
                .state
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.phase = phase;
            state.phase_name = phase_name.to_string();
            state.completed = 0;
            state.total = total_items;
            state.current_item.clear();
        }

        // Emit event
        self.emit(ProgressEvent::PhaseStarted {
            phase,
            phase_name: phase_name.to_string(),
            total_items,
        });
    }

    /// Complete current phase
    pub fn complete_phase(&self) {
        let phase = self
            .state
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .phase;
        let duration = self
            .start_time
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .map(|s| s.elapsed().as_secs())
            .unwrap_or(0);

        self.emit(ProgressEvent::PhaseCompleted {
            phase,
            duration_secs: duration,
        });
    }

    /// Update item progress
    pub fn update_progress(&self, completed: usize, current_item: &str) {
        self.items_processed.fetch_add(1, Ordering::SeqCst);

        let total_items = self.items_processed.load(Ordering::SeqCst);
        let elapsed: f32 = self
            .start_time
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .map(|s| s.elapsed().as_secs_f32())
            .unwrap_or(1.0);
        let throughput = total_items as f32 / elapsed.max(0.1);

        // Update state
        let (total, _phase) = {
            let mut state = self
                .state
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.completed = completed;
            state.current_item = current_item.to_string();
            state.throughput = throughput;
            state.elapsed_secs = elapsed as u64;

            // Calculate overall progress
            let phase_progress = if state.total > 0 {
                state.completed as f32 / state.total as f32
            } else {
                0.0
            };
            state.overall_progress =
                (state.phase as f32 - 1.0 + phase_progress) / self.total_phases as f32;

            (state.total, state.phase)
        };

        // Calculate ETA
        let remaining: usize = total.saturating_sub(completed);
        let eta_secs = if throughput > 0.0 && remaining > 0 {
            Some((remaining as f32 / throughput) as u64)
        } else {
            None
        };

        if let Some(eta) = eta_secs {
            self.state
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .eta_secs = Some(eta);
            self.emit(ProgressEvent::EtaUpdate {
                remaining_secs: eta,
            });
        }

        // Emit progress event
        self.emit(ProgressEvent::ItemProgress {
            completed,
            total,
            current_item: current_item.to_string(),
            throughput,
        });
    }

    /// Report an error
    pub fn report_error(&self, item: &str, error: &str, recoverable: bool) {
        self.emit(ProgressEvent::Error {
            item: item.to_string(),
            error: error.to_string(),
            recoverable,
        });

        self.emit(ProgressEvent::Message {
            level: if recoverable {
                MessageLevel::Warning
            } else {
                MessageLevel::Error
            },
            message: format!("{}: {}", item, error),
        });
    }

    /// Send a status message
    pub fn message(&self, level: MessageLevel, message: &str) {
        self.emit(ProgressEvent::Message {
            level,
            message: message.to_string(),
        });
    }

    /// Finish tracking
    pub fn finish(&self, success: bool, summary: &str) {
        let duration = self
            .start_time
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .map(|s| s.elapsed().as_secs())
            .unwrap_or(0);

        self.state
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_running = false;
        self.active.store(false, Ordering::SeqCst);

        self.emit(ProgressEvent::Finished {
            success,
            total_duration_secs: duration,
            summary: summary.to_string(),
        });
    }

    /// Check if tracking is active
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }
}

impl Clone for ProgressTracker {
    fn clone(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
            sender: self.sender.clone(),
            start_time: Arc::clone(&self.start_time),
            phase_times: Arc::clone(&self.phase_times),
            active: Arc::clone(&self.active),
            total_phases: self.total_phases,
            items_processed: Arc::clone(&self.items_processed),
        }
    }
}

/// Console progress renderer
pub struct ConsoleRenderer {
    tracker: ProgressTracker,
    show_spinner: bool,
    show_eta: bool,
}

impl ConsoleRenderer {
    pub fn new(tracker: ProgressTracker) -> Self {
        Self {
            tracker,
            show_spinner: true,
            show_eta: true,
        }
    }

    pub fn with_spinner(mut self, show: bool) -> Self {
        self.show_spinner = show;
        self
    }

    pub fn with_eta(mut self, show: bool) -> Self {
        self.show_eta = show;
        self
    }

    /// Render current state to console
    pub fn render(&self) -> String {
        let state = self.tracker.state();

        if !state.is_running {
            return String::new();
        }

        let spinner = if self.show_spinner {
            let chars = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
            let idx = (state.elapsed_secs as usize) % chars.len();
            format!("{} ", chars[idx])
        } else {
            String::new()
        };

        let progress_bar = render_progress_bar(state.completed, state.total, 30);

        let eta = if self.show_eta {
            state
                .eta_secs
                .map(|s| format!(" ETA: {}", format_duration(s)))
                .unwrap_or_default()
        } else {
            String::new()
        };

        let throughput = if state.throughput > 0.0 {
            format!(" ({:.1}/s)", state.throughput)
        } else {
            String::new()
        };

        format!(
            "{}[{}/7] {} {} {}/{}{}{}\n  {}",
            spinner,
            state.phase,
            state.phase_name,
            progress_bar,
            state.completed,
            state.total,
            throughput,
            eta,
            state.current_item
        )
    }

    /// Start rendering loop (non-blocking)
    pub fn start_render_loop(&self) -> tokio::task::JoinHandle<()> {
        let tracker = self.tracker.clone();
        let show_spinner = self.show_spinner;
        let show_eta = self.show_eta;

        tokio::spawn(async move {
            let renderer = ConsoleRenderer {
                tracker: tracker.clone(),
                show_spinner,
                show_eta,
            };

            while tracker.is_active() {
                let output = renderer.render();
                if !output.is_empty() {
                    print!("\r\x1B[K{}", output);
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }

            println!(); // Final newline
        })
    }
}

/// Render a simple progress bar
fn render_progress_bar(completed: usize, total: usize, width: usize) -> String {
    if total == 0 {
        return format!("[{}]", " ".repeat(width));
    }

    let progress = (completed as f32 / total as f32).min(1.0);
    let filled = (progress * width as f32) as usize;
    let empty = width.saturating_sub(filled);

    format!("[{}{}]", "█".repeat(filled), "░".repeat(empty))
}

/// Format duration as human-readable string
fn format_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_tracker_creation() {
        let tracker = ProgressTracker::new(7);
        assert!(!tracker.is_active());
        assert_eq!(tracker.state().phase, 0);
    }

    #[test]
    fn test_progress_tracker_start() {
        let tracker = ProgressTracker::new(7);
        tracker.start();
        assert!(tracker.is_active());
        assert!(tracker.state().is_running);
    }

    #[test]
    fn test_phase_tracking() {
        let tracker = ProgressTracker::new(7);
        tracker.start();
        tracker.start_phase(1, "Discovery", 100);

        let state = tracker.state();
        assert_eq!(state.phase, 1);
        assert_eq!(state.phase_name, "Discovery");
        assert_eq!(state.total, 100);
    }

    #[test]
    fn test_progress_update() {
        let tracker = ProgressTracker::new(7);
        tracker.start();
        tracker.start_phase(1, "Analysis", 10);
        tracker.update_progress(5, "file.rs");

        let state = tracker.state();
        assert_eq!(state.completed, 5);
        assert_eq!(state.current_item, "file.rs");
    }

    #[test]
    fn test_progress_bar_render() {
        assert_eq!(render_progress_bar(0, 10, 10), "[░░░░░░░░░░]");
        assert_eq!(render_progress_bar(5, 10, 10), "[█████░░░░░]");
        assert_eq!(render_progress_bar(10, 10, 10), "[██████████]");
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(30), "30s");
        assert_eq!(format_duration(90), "1m 30s");
        assert_eq!(format_duration(3700), "1h 1m");
    }
}
