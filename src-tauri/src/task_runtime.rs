use std::sync::{Arc, Mutex};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_cancel_for_non_active_run() {
        let registry = TaskRuntimeRegistry::new();
        registry
            .activate("group-a".to_string(), "run-a".to_string())
            .unwrap();

        let err = registry
            .require_active("group-b", "run-b")
            .expect_err("expected non-active run to be rejected");

        assert!(err.contains("Active run mismatch"));
    }

    #[test]
    fn clears_active_run_after_finish() {
        let registry = TaskRuntimeRegistry::new();
        registry
            .activate("group-a".to_string(), "run-a".to_string())
            .unwrap();

        assert!(registry.current().is_some());

        registry
            .clear("group-a", "run-a")
            .expect("expected active run to clear");

        assert!(registry.current().is_none());
    }

    #[test]
    fn clear_rejects_mismatched_run_without_clearing_active_execution() {
        let registry = TaskRuntimeRegistry::new();
        registry
            .activate("group-a".to_string(), "run-a".to_string())
            .unwrap();

        let err = registry
            .clear("group-a", "run-b")
            .expect_err("expected mismatched clear to fail");

        assert!(err.contains("Active run mismatch"));
        assert_eq!(
            registry.current(),
            Some(ActiveRunExecution {
                task_group_id: "group-a".to_string(),
                run_id: "run-a".to_string(),
            })
        );
    }

    #[test]
    fn activate_rejects_replacing_a_different_active_run() {
        let registry = TaskRuntimeRegistry::new();
        registry
            .activate("group-a".to_string(), "run-a".to_string())
            .unwrap();

        let err = registry
            .activate("group-b".to_string(), "run-b".to_string())
            .expect_err("expected replacing a different active run to fail");

        assert!(err.contains("Another task run is already active"));
        assert_eq!(
            registry.current(),
            Some(ActiveRunExecution {
                task_group_id: "group-a".to_string(),
                run_id: "run-a".to_string(),
            })
        );
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveRunExecution {
    pub task_group_id: String,
    pub run_id: String,
}

#[derive(Clone, Debug)]
pub struct TaskRuntimeRegistry {
    active_run: Arc<Mutex<Option<ActiveRunExecution>>>,
}

impl TaskRuntimeRegistry {
    pub fn new() -> Self {
        Self {
            active_run: Arc::new(Mutex::new(None)),
        }
    }

    pub fn activate(
        &self,
        task_group_id: String,
        run_id: String,
    ) -> Result<ActiveRunExecution, String> {
        let execution = ActiveRunExecution {
            task_group_id,
            run_id,
        };
        let mut active_run = self.active_run.lock().unwrap();

        match active_run.as_ref() {
            Some(active) if active == &execution => Ok(active.clone()),
            Some(active) => Err(format!(
                "Another task run is already active: expected {} / {}, got {} / {}",
                active.task_group_id, active.run_id, execution.task_group_id, execution.run_id
            )),
            None => {
                *active_run = Some(execution.clone());
                Ok(execution)
            }
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn require_active(
        &self,
        task_group_id: &str,
        run_id: &str,
    ) -> Result<ActiveRunExecution, String> {
        let active = self
            .active_run
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| "No active task run".to_string())?;

        if active.task_group_id != task_group_id || active.run_id != run_id {
            return Err(format!(
                "Active run mismatch: expected {} / {}, got {} / {}",
                active.task_group_id, active.run_id, task_group_id, run_id
            ));
        }

        Ok(active)
    }

    pub fn apply_if_active<T>(
        &self,
        task_group_id: &str,
        run_id: &str,
        action: impl FnOnce(&ActiveRunExecution) -> T,
    ) -> Result<T, String> {
        let active_run = self.active_run.lock().unwrap();
        let active = active_run
            .as_ref()
            .ok_or_else(|| "No active task run".to_string())?;

        if active.task_group_id != task_group_id || active.run_id != run_id {
            return Err(format!(
                "Active run mismatch: expected {} / {}, got {} / {}",
                active.task_group_id, active.run_id, task_group_id, run_id
            ));
        }

        Ok(action(active))
    }

    pub fn clear(&self, task_group_id: &str, run_id: &str) -> Result<(), String> {
        let mut active_run = self.active_run.lock().unwrap();
        let active = active_run
            .as_ref()
            .ok_or_else(|| "No active task run".to_string())?;

        if active.task_group_id != task_group_id || active.run_id != run_id {
            return Err(format!(
                "Active run mismatch: expected {} / {}, got {} / {}",
                active.task_group_id, active.run_id, task_group_id, run_id
            ));
        }

        *active_run = None;
        Ok(())
    }

    pub fn current(&self) -> Option<ActiveRunExecution> {
        self.active_run.lock().unwrap().clone()
    }
}
