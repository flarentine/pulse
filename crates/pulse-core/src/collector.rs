use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind, Users};

use crate::process::ProcessRow;
use crate::win;

const USER_REFRESH_TICKS: u32 = 30;

pub struct Collector {
    sys: System,
    users: Users,
    num_cpus: f32,
    ticks_since_user_refresh: u32,
}

impl Collector {
    pub fn new() -> anyhow::Result<Self> {
        let sys = System::new_all()?;
        let users = Users::new_with_refreshed_list()?;
        let num_cpus = sys.cpus().len().max(1) as f32;
        Ok(Self {
            sys,
            users,
            num_cpus,
            ticks_since_user_refresh: 0,
        })
    }

    /// CPU usage is a delta between ticks, so it reads 0 until the second call.
    pub fn tick(&mut self) -> Vec<ProcessRow> {
        // The default refresh kind skips cmd/user, which leaves processes
        // started after launch without either. Only fetch them once per process.
        let kind = ProcessRefreshKind::nothing()
            .with_cpu()
            .with_memory()
            .with_exe(UpdateKind::OnlyIfNotSet)
            .with_cmd(UpdateKind::OnlyIfNotSet)
            .with_user(UpdateKind::OnlyIfNotSet);
        self.sys
            .refresh_processes_specifics(ProcessesToUpdate::All, true, kind);

        self.ticks_since_user_refresh += 1;
        if self.ticks_since_user_refresh >= USER_REFRESH_TICKS {
            self.ticks_since_user_refresh = 0;
            if let Ok(users) = Users::new_with_refreshed_list() {
                self.users = users;
            }
        }

        let extras = win::process_extras();

        self.sys
            .processes()
            .iter()
            .map(|(pid, p)| {
                let pid = pid.as_u32();
                let extra = extras.get(&pid);
                ProcessRow {
                    pid,
                    parent_pid: p.parent().map(|ppid| ppid.as_u32()),
                    name: p.name().to_string_lossy().into_owned(),
                    exe_path: p.exe().map(|path| path.to_string_lossy().into_owned()),
                    cmd: p
                        .cmd()
                        .iter()
                        .map(|s| s.to_string_lossy())
                        .collect::<Vec<_>>()
                        .join(" "),
                    user: p
                        .user_id()
                        .and_then(|uid| self.users.get_user_by_id(uid))
                        .map(|u| u.name().to_string()),
                    cpu_percent: p.cpu_usage() / self.num_cpus,
                    memory_bytes: p.memory(),
                    // sysinfo fills "virtual memory" from PrivateUsage on Windows.
                    private_bytes: p.virtual_memory(),
                    threads: extra.map_or(0, |e| e.threads),
                    handles: win::handle_count(pid),
                    priority: extra.and_then(|e| e.priority),
                    start_time_secs: p.start_time(),
                    run_time_secs: p.run_time(),
                }
            })
            .collect()
    }
}
