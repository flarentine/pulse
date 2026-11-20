use std::os::windows::process::CommandExt;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows::Win32::System::Threading::{
    ABOVE_NORMAL_PRIORITY_CLASS, BELOW_NORMAL_PRIORITY_CLASS, HIGH_PRIORITY_CLASS,
    IDLE_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS, OpenThread, PROCESS_CREATION_FLAGS,
    PROCESS_SET_INFORMATION, PROCESS_TERMINATE, REALTIME_PRIORITY_CLASS, ResumeThread,
    SetPriorityClass, SuspendThread, THREAD_SUSPEND_RESUME, TerminateProcess,
};

use crate::win::{OwnedHandle, open_process};

// Ending any of these takes Windows down with it (or forces a restart).
const CRITICAL_PROCESSES: &[&str] = &[
    "system",
    "registry",
    "smss.exe",
    "csrss.exe",
    "wininit.exe",
    "winlogon.exe",
    "services.exe",
    "lsass.exe",
];

pub fn is_critical(pid: u32, name: &str) -> bool {
    pid == 4 || CRITICAL_PROCESSES.contains(&name.to_ascii_lowercase().as_str())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Priority {
    Idle,
    BelowNormal,
    Normal,
    AboveNormal,
    High,
    Realtime,
}

impl Priority {
    pub const ALL: [Priority; 6] = [
        Priority::Idle,
        Priority::BelowNormal,
        Priority::Normal,
        Priority::AboveNormal,
        Priority::High,
        Priority::Realtime,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Priority::Idle => "Idle",
            Priority::BelowNormal => "Below normal",
            Priority::Normal => "Normal",
            Priority::AboveNormal => "Above normal",
            Priority::High => "High",
            Priority::Realtime => "Realtime",
        }
    }

    fn flags(self) -> PROCESS_CREATION_FLAGS {
        match self {
            Priority::Idle => IDLE_PRIORITY_CLASS,
            Priority::BelowNormal => BELOW_NORMAL_PRIORITY_CLASS,
            Priority::Normal => NORMAL_PRIORITY_CLASS,
            Priority::AboveNormal => ABOVE_NORMAL_PRIORITY_CLASS,
            Priority::High => HIGH_PRIORITY_CLASS,
            Priority::Realtime => REALTIME_PRIORITY_CLASS,
        }
    }

    /// From the base thread priority Toolhelp reports for each class.
    pub(crate) fn from_base(base: i32) -> Option<Self> {
        Some(match base {
            4 => Priority::Idle,
            6 => Priority::BelowNormal,
            8 => Priority::Normal,
            10 => Priority::AboveNormal,
            13 => Priority::High,
            24 => Priority::Realtime,
            _ => return None,
        })
    }
}

pub fn kill_process(pid: u32) -> anyhow::Result<()> {
    let handle = open_process(pid, PROCESS_TERMINATE)?;
    unsafe { TerminateProcess(handle.0, 1)? };
    Ok(())
}

pub fn set_priority(pid: u32, priority: Priority) -> anyhow::Result<()> {
    let handle = open_process(pid, PROCESS_SET_INFORMATION)?;
    unsafe { SetPriorityClass(handle.0, priority.flags())? };
    Ok(())
}

pub fn suspend_process(pid: u32) -> anyhow::Result<()> {
    // Suspending ourselves would freeze the UI thread mid-loop with no way back.
    if pid == std::process::id() {
        anyhow::bail!("can't suspend Pulse itself");
    }
    apply_to_threads(pid, |thread| unsafe { SuspendThread(thread) })
}

pub fn resume_process(pid: u32) -> anyhow::Result<()> {
    apply_to_threads(pid, |thread| unsafe { ResumeThread(thread) })
}

// Uses Toolhelp + per-thread Suspend/ResumeThread rather than the
// undocumented NtSuspendProcess, at the cost of not being atomic.
fn apply_to_threads(pid: u32, mut op: impl FnMut(HANDLE) -> u32) -> anyhow::Result<()> {
    let snapshot = OwnedHandle(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0)? });
    let mut entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };

    let mut found = 0;
    let mut applied = 0;
    let mut more = unsafe { Thread32First(snapshot.0, &mut entry) }.is_ok();
    while more {
        if entry.th32OwnerProcessID == pid {
            found += 1;
            if let Ok(thread) =
                unsafe { OpenThread(THREAD_SUSPEND_RESUME, false, entry.th32ThreadID) }
            {
                let thread = OwnedHandle(thread);
                if op(thread.0) != u32::MAX {
                    applied += 1;
                }
            }
        }
        more = unsafe { Thread32Next(snapshot.0, &mut entry) }.is_ok();
    }

    match (found, applied) {
        (0, _) => anyhow::bail!("no threads found"),
        (_, 0) => anyhow::bail!("access denied"),
        (f, a) if a < f => anyhow::bail!("only {a} of {f} threads affected"),
        _ => Ok(()),
    }
}

pub fn open_file_location(path: &str) -> anyhow::Result<()> {
    // Explorer does its own command-line parsing, so the quoting Command adds
    // for paths with spaces breaks /select. Windows paths can't contain '"'.
    std::process::Command::new("explorer.exe")
        .raw_arg(format!("/select,\"{path}\""))
        .spawn()?;
    Ok(())
}
