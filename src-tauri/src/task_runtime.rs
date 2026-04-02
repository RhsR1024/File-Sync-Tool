#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_cancel_for_non_active_run() {
        let registry = TaskRuntimeRegistry::new();
        registry.activate("group-a".to_string(), "run-a".to_string());

        let err = registry
            .require_active("group-b", "run-b")
            .expect_err("expected non-active run to be rejected");

        assert!(err.contains("Active run mismatch"));
    }

    #[test]
    fn clears_active_run_after_finish() {
        let registry = TaskRuntimeRegistry::new();
        registry.activate("group-a".to_string(), "run-a".to_string());

        assert!(registry.current().is_some());

        registry
            .clear("group-a", "run-a")
            .expect("expected active run to clear");

        assert!(registry.current().is_none());
    }
}
use std::sync::Mutex;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveRunExecution {
    pub task_group_id: String,
    pub run_id: String,
}

#[derive(Debug)]
pub struct TaskRuntimeRegistry {
    active_run: Mutex<Option<ActiveRunExecution>>,
}

impl TaskRuntimeRegistry {
    pub fn new() -> Self {
        Self {
            active_run: Mutex::new(None),
        }
    }

    pub fn activate(&self, task_group_id: String, run_id: String) -> ActiveRunExecution {
        let execution = ActiveRunExecution {
            task_group_id,
            run_id,
        };
        *self.active_run.lock().unwrap() = Some(execution.clone());
        execution
    }

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

    pub fn clear(&self, task_group_id: &str, run_id: &str) -> Result<(), String> {
        self.require_active(task_group_id, run_id)?;
        *self.active_run.lock().unwrap() = None;
        Ok(())
    }

    pub fn current(&self) -> Option<ActiveRunExecution> {
        self.active_run.lock().unwrap().clone()
    }
}
