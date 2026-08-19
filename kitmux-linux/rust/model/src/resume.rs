use crate::{PaneId, SurfaceId, valid_resume_command};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResumeCommandIdentity {
    pub pane_id: PaneId,
    pub surface_id: SurfaceId,
    pub command: String,
    pub cwd: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResumeCommandCurrentState {
    pub pane_id: PaneId,
    pub surface_id: SurfaceId,
    pub command: Option<String>,
    pub cwd: Option<String>,
    pub is_eligible: bool,
}

#[derive(Clone, Debug, Default)]
pub struct ResumeCommandSelectionPolicy {
    displayed_rows: Vec<ResumeCommandIdentity>,
    selected: HashSet<SurfaceId>,
}

impl ResumeCommandSelectionPolicy {
    #[must_use]
    pub fn new(rows: Vec<ResumeCommandIdentity>) -> Self {
        let mut seen = HashSet::new();
        Self {
            displayed_rows: rows
                .into_iter()
                .filter(|row| seen.insert(row.surface_id))
                .collect(),
            selected: HashSet::new(),
        }
    }

    #[must_use]
    pub fn displayed_rows(&self) -> &[ResumeCommandIdentity] {
        &self.displayed_rows
    }

    #[must_use]
    pub fn selected_row_ids(&self) -> Vec<SurfaceId> {
        self.displayed_rows
            .iter()
            .filter(|row| self.selected.contains(&row.surface_id))
            .map(|row| row.surface_id)
            .collect()
    }

    #[must_use]
    pub fn is_selected(&self, surface_id: SurfaceId) -> bool {
        self.selected.contains(&surface_id)
    }

    pub fn set_selected(&mut self, surface_id: SurfaceId, selected: bool) {
        if !self
            .displayed_rows
            .iter()
            .any(|row| row.surface_id == surface_id)
        {
            return;
        }
        if selected {
            self.selected.insert(surface_id);
        } else {
            self.selected.remove(&surface_id);
        }
    }

    #[must_use]
    pub fn executable_rows(
        &self,
        current: &[ResumeCommandCurrentState],
    ) -> Vec<ResumeCommandIdentity> {
        if self.selected.is_empty() {
            return Vec::new();
        }
        let mut by_id = HashMap::new();
        let mut duplicates = HashSet::new();
        for state in current {
            if by_id.insert(state.surface_id, state).is_some() {
                duplicates.insert(state.surface_id);
            }
        }
        self.displayed_rows
            .iter()
            .filter(|displayed| {
                if !self.selected.contains(&displayed.surface_id)
                    || duplicates.contains(&displayed.surface_id)
                    || valid_resume_command(Some(&displayed.command)).as_deref()
                        != Some(displayed.command.as_str())
                {
                    return false;
                }
                let Some(live) = by_id.get(&displayed.surface_id) else {
                    return false;
                };
                live.pane_id == displayed.pane_id
                    && live.is_eligible
                    && live.cwd == displayed.cwd
                    && valid_resume_command(live.command.as_deref()).as_deref()
                        == Some(displayed.command.as_str())
            })
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(
        pane_id: PaneId,
        surface_id: SurfaceId,
        command: &str,
        cwd: &str,
    ) -> ResumeCommandIdentity {
        ResumeCommandIdentity {
            pane_id,
            surface_id,
            command: command.to_owned(),
            cwd: Some(cwd.to_owned()),
        }
    }

    fn current(
        pane_id: PaneId,
        surface_id: SurfaceId,
        command: &str,
        cwd: &str,
    ) -> ResumeCommandCurrentState {
        ResumeCommandCurrentState {
            pane_id,
            surface_id,
            command: Some(command.to_owned()),
            cwd: Some(cwd.to_owned()),
            is_eligible: true,
        }
    }

    #[test]
    fn rows_start_unchecked_and_only_unchanged_selected_rows_execute() {
        let pane_a = PaneId::new();
        let pane_b = PaneId::new();
        let surface_a = SurfaceId::new();
        let surface_b = SurfaceId::new();
        let mut policy = ResumeCommandSelectionPolicy::new(vec![
            row(pane_a, surface_a, "echo a", "/tmp/a"),
            row(pane_b, surface_b, "echo b", "/tmp/b"),
        ]);
        assert!(policy.selected_row_ids().is_empty());
        policy.set_selected(surface_a, true);
        policy.set_selected(surface_b, true);
        let executable = policy.executable_rows(&[
            current(pane_a, surface_a, "echo a", "/tmp/a"),
            current(pane_b, surface_b, "changed", "/tmp/b"),
        ]);
        assert_eq!(executable.len(), 1);
        assert_eq!(executable[0].pane_id, pane_a);
    }

    #[test]
    fn duplicate_live_identity_is_rejected() {
        let pane = PaneId::new();
        let surface = SurfaceId::new();
        let mut policy =
            ResumeCommandSelectionPolicy::new(vec![row(pane, surface, "echo", "/tmp")]);
        policy.set_selected(surface, true);
        let current = current(pane, surface, "echo", "/tmp");
        assert!(
            policy
                .executable_rows(&[current.clone(), current])
                .is_empty()
        );
    }

    #[test]
    fn same_pane_surfaces_remain_independent_rows() {
        let pane = PaneId::new();
        let surface_a = SurfaceId::new();
        let surface_b = SurfaceId::new();
        let mut policy = ResumeCommandSelectionPolicy::new(vec![
            row(pane, surface_a, "echo a", "/tmp/a"),
            row(pane, surface_b, "echo b", "/tmp/b"),
        ]);
        assert_eq!(policy.displayed_rows().len(), 2);
        policy.set_selected(surface_a, true);
        policy.set_selected(surface_b, true);
        let executable = policy.executable_rows(&[
            current(pane, surface_a, "echo a", "/tmp/a"),
            current(pane, surface_b, "echo b", "/tmp/b"),
        ]);
        assert_eq!(executable.len(), 2);
    }
}
