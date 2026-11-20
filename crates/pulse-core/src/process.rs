use crate::actions::Priority;

#[derive(Clone, Debug)]
pub struct ProcessRow {
    pub pid: u32,
    pub parent_pid: Option<u32>,
    pub name: String,
    pub exe_path: Option<String>,
    pub cmd: String,
    pub user: Option<String>,
    /// Share of the whole machine, 0-100.
    pub cpu_percent: f32,
    pub memory_bytes: u64,
    pub private_bytes: u64,
    pub threads: u32,
    /// None when the process can't be opened (protected processes).
    pub handles: Option<u32>,
    pub priority: Option<Priority>,
    /// Unix seconds. 0 when Windows won't tell us (protected processes).
    pub start_time_secs: u64,
    pub run_time_secs: u64,
}

impl ProcessRow {
    pub fn display_name(&self) -> &str {
        if self.name.is_empty() {
            self.exe_path.as_deref().unwrap_or("(unknown)")
        } else {
            &self.name
        }
    }
}

#[cfg(test)]
pub(crate) fn test_row(pid: u32, parent_pid: Option<u32>, name: &str, start: u64) -> ProcessRow {
    ProcessRow {
        pid,
        parent_pid,
        name: name.into(),
        exe_path: None,
        cmd: String::new(),
        user: None,
        cpu_percent: 0.0,
        memory_bytes: 0,
        private_bytes: 0,
        threads: 1,
        handles: None,
        priority: None,
        start_time_secs: start,
        run_time_secs: 0,
    }
}
