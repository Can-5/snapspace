//! SnapSpace: workspace-profile engine for Orbit.
//!
//! SnapSpace captures *intent* (which apps, where, on which display) — it never talks to
//! a window manager. When Orbit wants to restore a profile it hands `plan_restore` the
//! set of currently-running apps and the connected displays; the result tells the user
//! exactly what will and won't happen. Nothing is ever silently skipped.
//!
//! Honesty contract: restoring is best-effort and platform-limited (Wayland compositors
//! may refuse to place foreign windows). Anything SnapSpace cannot verify is reported,
//! never pretended.

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SnapSpaceError {
    #[error("profile name must not be empty")]
    EmptyName,
}

/// Rectangle in *physical* pixels, origin top-left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl WindowRect {
    pub fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self { x, y, width, height }
    }

    pub fn right(&self) -> i64 {
        self.x as i64 + self.width as i64
    }

    pub fn bottom(&self) -> i64 {
        self.y as i64 + self.height as i64
    }

    pub fn intersects(&self, other: &WindowRect) -> bool {
        (self.x as i64) < other.right()
            && (other.x as i64) < self.right()
            && (self.y as i64) < other.bottom()
            && (other.y as i64) < self.bottom()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayBounds {
    pub id: u32,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl DisplayBounds {
    pub fn contains(&self, r: &WindowRect) -> bool {
        r.x >= self.x
            && r.y >= self.y
            && r.right() <= self.x as i64 + self.width as i64
            && r.bottom() <= self.y as i64 + self.height as i64
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedWindow {
    /// Stable-ish app identifier (desktop-file id / bundle id), e.g. "firefox.desktop".
    pub app_id: String,
    /// Cosmetic hint only; never used for matching logic.
    pub title_hint: Option<String>,
    pub rect: WindowRect,
    pub workspace: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceProfile {
    pub name: String,
    pub windows: Vec<ManagedWindow>,
    /// Links/notes the user attached to this workspace (opened with explicit action).
    pub links: Vec<String>,
    pub tags: Vec<String>,
}

impl WorkspaceProfile {
    pub fn new(name: impl Into<String>) -> Result<Self, SnapSpaceError> {
        let name = name.into();
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err(SnapSpaceError::EmptyName);
        }
        Ok(Self { name, windows: Vec::new(), links: Vec::new(), tags: Vec::new() })
    }

    pub fn add_window(&mut self, w: ManagedWindow) {
        self.windows.push(w);
    }

    /// Convenience constructor: build and push a `ManagedWindow` in one call.
    pub fn add_window_at(
        &mut self,
        rect: WindowRect,
        app_id: impl Into<String>,
        title_hint: Option<String>,
        workspace: Option<u32>,
    ) {
        self.windows.push(ManagedWindow { app_id: app_id.into(), title_hint, rect, workspace });
    }

    /// Sorted, unique workspace ids referenced by this profile's windows.
    pub fn workspace_ids(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self.windows.iter().filter_map(|w| w.workspace).collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }
}

/// Borrowed views of the windows assigned to workspace `ws`, in profile order.
pub fn windows_in_workspace<'a>(profile: &'a WorkspaceProfile, ws: u32) -> Vec<&'a ManagedWindow> {
    profile.windows.iter().filter(|w| w.workspace == Some(ws)).collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValidationWarning {
    /// Window rectangle lies (partially) outside every known display.
    OutOfBounds { window_index: usize, detail: String },
    /// Two windows on the same workspace overlap.
    Overlapping { a: usize, b: usize, workspace: Option<u32> },
    /// Profile has no windows.
    Empty,
}

/// Static validation of a profile against known displays. Returns warnings, never errors —
/// an odd layout is a warning, not a failure.
pub fn validate_profile(profile: &WorkspaceProfile, displays: &[DisplayBounds]) -> Vec<ValidationWarning> {
    let mut warnings = Vec::new();
    if profile.windows.is_empty() {
        warnings.push(ValidationWarning::Empty);
    }
    for (i, w) in profile.windows.iter().enumerate() {
        let on_some_display = displays.iter().any(|d| d.contains(&w.rect));
        if !on_some_display {
            warnings.push(ValidationWarning::OutOfBounds {
                window_index: i,
                detail: format!(
                    "rect({},{},{}x{}) not fully inside any known display",
                    w.rect.x, w.rect.y, w.rect.width, w.rect.height
                ),
            });
        }
    }
    // Overlap check only within the same workspace.
    for i in 0..profile.windows.len() {
        for j in (i + 1)..profile.windows.len() {
            let a = &profile.windows[i];
            let b = &profile.windows[j];
            if a.workspace == b.workspace && a.rect.intersects(&b.rect) {
                warnings.push(ValidationWarning::Overlapping { a: i, b: j, workspace: a.workspace });
            }
        }
    }
    warnings
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RestoreOutcome {
    /// App is running and the layout is placeable on a known display.
    Ready,
    /// App was not detected among running/ known applications.
    MissingApp,
    /// Requested rectangle is not fully inside any known display.
    OutOfBounds { detail: String },
    /// Platform refused (e.g. Wayland compositor denied foreign placement).
    Refused { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestorePlanEntry {
    pub window_index: usize,
    pub app_id: String,
    pub outcome: RestoreOutcome,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestorePlan {
    pub entries: Vec<RestorePlanEntry>,
}

impl RestorePlan {
    pub fn ready_count(&self) -> usize {
        self.entries.iter().filter(|e| e.outcome == RestoreOutcome::Ready).count()
    }
    pub fn blocked_count(&self) -> usize {
        self.entries.len() - self.ready_count()
    }
    pub fn is_clean(&self) -> bool {
        self.entries.iter().all(|e| e.outcome == RestoreOutcome::Ready)
    }
}

/// Build a restore plan. `running_apps` is the set of app ids Orbit detected (from
/// whatever the platform honestly exposes). `displays` is the current display topology.
/// Per-window outcomes are independent — one bad window never blocks the others.
pub fn plan_restore(
    profile: &WorkspaceProfile,
    running_apps: &[String],
    displays: &[DisplayBounds],
) -> RestorePlan {
    let mut plan = RestorePlan::default();
    for (i, w) in profile.windows.iter().enumerate() {
        let outcome = if !running_apps.iter().any(|a| a == &w.app_id) {
            RestoreOutcome::MissingApp
        } else if !displays.iter().any(|d| d.contains(&w.rect)) {
            RestoreOutcome::OutOfBounds {
                detail: format!(
                    "rect({},{},{}x{}) outside all displays",
                    w.rect.x, w.rect.y, w.rect.width, w.rect.height
                ),
            }
        } else {
            RestoreOutcome::Ready
        };
        plan.entries.push(RestorePlanEntry { window_index: i, app_id: w.app_id.clone(), outcome });
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display() -> DisplayBounds {
        DisplayBounds { id: 0, x: 0, y: 0, width: 1920, height: 1080 }
    }

    fn sample_profile() -> WorkspaceProfile {
        let mut p = WorkspaceProfile::new("Work").unwrap();
        p.add_window(ManagedWindow {
            app_id: "firefox.desktop".into(),
            title_hint: Some("Docs".into()),
            rect: WindowRect::new(0, 0, 960, 1080),
            workspace: Some(1),
        });
        p.add_window(ManagedWindow {
            app_id: "code.desktop".into(),
            title_hint: None,
            rect: WindowRect::new(960, 0, 960, 1080),
            workspace: Some(1),
        });
        p
    }

    #[test]
    fn empty_name_rejected() {
        assert_eq!(WorkspaceProfile::new("   ").unwrap_err(), SnapSpaceError::EmptyName);
    }

    #[test]
    fn validate_clean_profile_has_no_warnings() {
        let p = sample_profile();
        let w = validate_profile(&p, &[display()]);
        assert!(w.is_empty(), "unexpected warnings: {w:?}");
    }

    #[test]
    fn out_of_bounds_window_is_flagged() {
        let mut p = sample_profile();
        p.add_window(ManagedWindow {
            app_id: "gimp.desktop".into(),
            title_hint: None,
            rect: WindowRect::new(5000, 5000, 800, 600),
            workspace: Some(2),
        });
        let warnings = validate_profile(&p, &[display()]);
        assert!(warnings.iter().any(|w| matches!(w, ValidationWarning::OutOfBounds { window_index: 2, .. })));
    }

    #[test]
    fn overlapping_windows_flagged_only_on_same_workspace() {
        let mut p = WorkspaceProfile::new("X").unwrap();
        p.add_window(ManagedWindow { app_id: "a".into(), title_hint: None, rect: WindowRect::new(0, 0, 100, 100), workspace: Some(1) });
        p.add_window(ManagedWindow { app_id: "b".into(), title_hint: None, rect: WindowRect::new(10, 10, 100, 100), workspace: Some(1) });
        p.add_window(ManagedWindow { app_id: "c".into(), title_hint: None, rect: WindowRect::new(10, 10, 100, 100), workspace: Some(2) });
        let warnings = validate_profile(&p, &[display()]);
        let overlaps: Vec<_> = warnings.iter().filter(|w| matches!(w, ValidationWarning::Overlapping { .. })).collect();
        assert_eq!(overlaps.len(), 1);
    }

    #[test]
    fn windows_in_workspace_filters() {
        let p = sample_profile();
        let ws1 = windows_in_workspace(&p, 1);
        assert_eq!(ws1.len(), 2);
        assert!(windows_in_workspace(&p, 2).is_empty());
        assert!(windows_in_workspace(&p, 99).is_empty());
    }

    #[test]
    fn workspace_ids_sorted_unique() {
        let mut p = WorkspaceProfile::new("Multi").unwrap();
        assert!(p.workspace_ids().is_empty());
        p.add_window_at(WindowRect::new(0, 0, 100, 100), "a", None, Some(3));
        p.add_window_at(WindowRect::new(0, 0, 100, 100), "b", Some("hint".into()), Some(1));
        p.add_window_at(WindowRect::new(0, 0, 100, 100), "c", None, Some(3));
        p.add_window_at(WindowRect::new(0, 0, 100, 100), "d", None, None); // no workspace — excluded
        assert_eq!(p.workspace_ids(), vec![1, 3]);
    }

    #[test]
    fn add_window_at_sets_fields() {
        let mut p = WorkspaceProfile::new("Added").unwrap();
        p.add_window_at(WindowRect::new(10, 20, 300, 200), "term.desktop", Some("Shell".into()), Some(7));
        let w = &p.windows[0];
        assert_eq!(w.app_id, "term.desktop");
        assert_eq!(w.title_hint.as_deref(), Some("Shell"));
        assert_eq!(w.workspace, Some(7));
        assert_eq!(w.rect, WindowRect::new(10, 20, 300, 200));
    }

    #[test]
    fn restore_plan_reports_missing_apps_and_oob() {
        let mut p = sample_profile();
        p.add_window(ManagedWindow {
            app_id: "missing.desktop".into(),
            title_hint: None,
            rect: WindowRect::new(0, 0, 100, 100),
            workspace: Some(1),
        });
        let running = vec!["firefox.desktop".to_string(), "code.desktop".to_string()];
        let plan = plan_restore(&p, &running, &[display()]);
        assert_eq!(plan.entries[0].outcome, RestoreOutcome::Ready);
        assert_eq!(plan.entries[1].outcome, RestoreOutcome::Ready);
        assert_eq!(plan.entries[2].outcome, RestoreOutcome::MissingApp);
        assert_eq!(plan.ready_count(), 2);
        assert_eq!(plan.blocked_count(), 1);
        assert!(!plan.is_clean());
    }

    #[test]
    fn restore_plan_out_of_bounds_when_display_shrinks() {
        let p = sample_profile();
        let small = [DisplayBounds { id: 0, x: 0, y: 0, width: 1280, height: 720 }];
        let running = vec!["firefox.desktop".to_string(), "code.desktop".to_string()];
        let plan = plan_restore(&p, &running, &small);
        assert!(matches!(plan.entries[0].outcome, RestoreOutcome::OutOfBounds { .. }));
    }
}
