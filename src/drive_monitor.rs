use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc;

use crate::types::DriveEvent;

/// Tracks known drives and their disc state, emits DriveEvents on changes.
pub struct DriveMonitor {
    known_drives: HashSet<PathBuf>,
    disc_labels: HashMap<PathBuf, String>,
    tx: mpsc::Sender<DriveEvent>,
}

impl DriveMonitor {
    pub fn new(tx: mpsc::Sender<DriveEvent>) -> Self {
        Self {
            known_drives: HashSet::new(),
            disc_labels: HashMap::new(),
            tx,
        }
    }

    /// Compare current drive state against known state and emit events.
    pub fn diff_and_emit(
        &mut self,
        current_drives: Vec<PathBuf>,
        get_label: &dyn Fn(&PathBuf) -> String,
    ) {
        let current_set: HashSet<PathBuf> = current_drives.iter().cloned().collect();

        // Detect disappeared drives
        let disappeared: Vec<PathBuf> = self
            .known_drives
            .difference(&current_set)
            .cloned()
            .collect();
        for drive in &disappeared {
            let _ = self.tx.send(DriveEvent::DriveDisappeared(drive.clone()));
            self.disc_labels.remove(drive);
        }

        // Detect new drives
        let appeared: Vec<PathBuf> = current_set
            .difference(&self.known_drives)
            .cloned()
            .collect();
        for drive in &appeared {
            let _ = self.tx.send(DriveEvent::DriveAppeared(drive.clone()));
        }

        // Check disc state for all current drives
        for drive in &current_drives {
            let label = get_label(drive);
            let old_label = self.disc_labels.get(drive).cloned().unwrap_or_default();

            if old_label.is_empty() && !label.is_empty() {
                let _ = self
                    .tx
                    .send(DriveEvent::DiscInserted(drive.clone(), label.clone()));
            } else if !old_label.is_empty() && label.is_empty() {
                let _ = self.tx.send(DriveEvent::DiscEjected(drive.clone()));
            } else if !old_label.is_empty() && !label.is_empty() && old_label != label {
                let _ = self.tx.send(DriveEvent::DiscEjected(drive.clone()));
                let _ = self
                    .tx
                    .send(DriveEvent::DiscInserted(drive.clone(), label.clone()));
            }

            self.disc_labels.insert(drive.clone(), label);
        }

        self.known_drives = current_set;
    }

    /// Spawn the monitor in a background thread, polling every `interval`.
    pub fn spawn(interval: std::time::Duration, tx: mpsc::Sender<DriveEvent>) {
        std::thread::Builder::new()
            .name("drive-monitor".into())
            .spawn(move || {
                let mut monitor = DriveMonitor::new(tx);
                loop {
                    let drives = crate::disc::detect_optical_drives();
                    monitor.diff_and_emit(drives, &|d| {
                        crate::disc::get_volume_label(&d.to_string_lossy())
                    });
                    std::thread::sleep(interval);
                }
            })
            .expect("failed to spawn drive monitor thread");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect_events(rx: &mpsc::Receiver<DriveEvent>) -> Vec<String> {
        let mut events = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            let desc = match ev {
                DriveEvent::DriveAppeared(p) => format!("appeared:{}", p.display()),
                DriveEvent::DriveDisappeared(p) => format!("disappeared:{}", p.display()),
                DriveEvent::DiscInserted(p, l) => format!("inserted:{}:{}", p.display(), l),
                DriveEvent::DiscEjected(p) => format!("ejected:{}", p.display()),
            };
            events.push(desc);
        }
        events
    }

    #[test]
    fn test_new_drive_appears() {
        let (tx, rx) = mpsc::channel();
        let mut monitor = DriveMonitor::new(tx);
        let drives = vec![PathBuf::from("/dev/sr0")];
        monitor.diff_and_emit(drives, &|_| String::new());
        let events = collect_events(&rx);
        assert_eq!(events, vec!["appeared:/dev/sr0"]);
    }

    #[test]
    fn test_drive_disappears() {
        let (tx, rx) = mpsc::channel();
        let mut monitor = DriveMonitor::new(tx);
        monitor.diff_and_emit(vec![PathBuf::from("/dev/sr0")], &|_| String::new());
        let _ = collect_events(&rx);
        monitor.diff_and_emit(vec![], &|_| String::new());
        let events = collect_events(&rx);
        assert_eq!(events, vec!["disappeared:/dev/sr0"]);
    }

    #[test]
    fn test_disc_inserted() {
        let (tx, rx) = mpsc::channel();
        let mut monitor = DriveMonitor::new(tx);
        monitor.diff_and_emit(vec![PathBuf::from("/dev/sr0")], &|_| String::new());
        let _ = collect_events(&rx);
        monitor.diff_and_emit(vec![PathBuf::from("/dev/sr0")], &|_| {
            "BREAKING_BAD_S1D1".into()
        });
        let events = collect_events(&rx);
        assert_eq!(events, vec!["inserted:/dev/sr0:BREAKING_BAD_S1D1"]);
    }

    #[test]
    fn test_disc_ejected() {
        let (tx, rx) = mpsc::channel();
        let mut monitor = DriveMonitor::new(tx);
        monitor.diff_and_emit(vec![PathBuf::from("/dev/sr0")], &|_| "DISC_LABEL".into());
        let _ = collect_events(&rx);
        monitor.diff_and_emit(vec![PathBuf::from("/dev/sr0")], &|_| String::new());
        let events = collect_events(&rx);
        assert_eq!(events, vec!["ejected:/dev/sr0"]);
    }

    #[test]
    fn test_no_change_no_events() {
        let (tx, rx) = mpsc::channel();
        let mut monitor = DriveMonitor::new(tx);
        monitor.diff_and_emit(vec![PathBuf::from("/dev/sr0")], &|_| "LABEL".into());
        let _ = collect_events(&rx);
        monitor.diff_and_emit(vec![PathBuf::from("/dev/sr0")], &|_| "LABEL".into());
        let events = collect_events(&rx);
        assert!(events.is_empty());
    }

    #[test]
    fn test_multiple_drives() {
        let (tx, rx) = mpsc::channel();
        let mut monitor = DriveMonitor::new(tx);
        let drives = vec![PathBuf::from("/dev/sr0"), PathBuf::from("/dev/sr1")];
        monitor.diff_and_emit(drives, &|d| {
            if d == &PathBuf::from("/dev/sr0") {
                "DISC_A".into()
            } else {
                String::new()
            }
        });
        let events = collect_events(&rx);
        assert!(events.contains(&"appeared:/dev/sr0".to_string()));
        assert!(events.contains(&"appeared:/dev/sr1".to_string()));
        assert!(events.contains(&"inserted:/dev/sr0:DISC_A".to_string()));
    }

    #[test]
    fn test_drive_disappears_with_disc() {
        let (tx, rx) = mpsc::channel();
        let mut monitor = DriveMonitor::new(tx);
        monitor.diff_and_emit(vec![PathBuf::from("/dev/sr0")], &|_| "LABEL".into());
        let _ = collect_events(&rx);
        monitor.diff_and_emit(vec![], &|_| String::new());
        let events = collect_events(&rx);
        assert_eq!(events, vec!["disappeared:/dev/sr0"]);
    }

    #[test]
    fn test_disc_swap_emits_eject_and_insert() {
        let (tx, rx) = mpsc::channel();
        let mut monitor = DriveMonitor::new(tx);
        monitor.diff_and_emit(vec![PathBuf::from("/dev/sr0")], &|_| "DISC_A".into());
        let _ = collect_events(&rx);

        monitor.diff_and_emit(vec![PathBuf::from("/dev/sr0")], &|_| "DISC_B".into());
        let events = collect_events(&rx);
        assert_eq!(
            events,
            vec!["ejected:/dev/sr0", "inserted:/dev/sr0:DISC_B"],
            "disc swap should emit eject then insert"
        );
    }

    #[test]
    fn test_same_disc_no_events() {
        let (tx, rx) = mpsc::channel();
        let mut monitor = DriveMonitor::new(tx);
        monitor.diff_and_emit(vec![PathBuf::from("/dev/sr0")], &|_| "SAME_DISC".into());
        let _ = collect_events(&rx);

        monitor.diff_and_emit(vec![PathBuf::from("/dev/sr0")], &|_| "SAME_DISC".into());
        let events = collect_events(&rx);
        assert!(events.is_empty(), "same disc should produce no events");
    }
}
