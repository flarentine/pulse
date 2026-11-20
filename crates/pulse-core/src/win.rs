use std::collections::HashMap;

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    GetProcessHandleCount, OpenProcess, PROCESS_ACCESS_RIGHTS, PROCESS_QUERY_LIMITED_INFORMATION,
};

use crate::actions::Priority;

pub(crate) struct OwnedHandle(pub HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

pub(crate) fn open_process(pid: u32, access: PROCESS_ACCESS_RIGHTS) -> anyhow::Result<OwnedHandle> {
    let handle = unsafe { OpenProcess(access, false, pid)? };
    Ok(OwnedHandle(handle))
}

pub(crate) struct ProcessExtras {
    pub threads: u32,
    pub priority: Option<Priority>,
}

/// Thread counts and priorities for every process from one Toolhelp snapshot.
/// Works for protected processes too, since it doesn't open them.
pub(crate) fn process_extras() -> HashMap<u32, ProcessExtras> {
    let mut out = HashMap::new();
    let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return out;
    };
    let snapshot = OwnedHandle(snapshot);
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };

    let mut more = unsafe { Process32FirstW(snapshot.0, &mut entry) }.is_ok();
    while more {
        out.insert(
            entry.th32ProcessID,
            ProcessExtras {
                threads: entry.cntThreads,
                priority: Priority::from_base(entry.pcPriClassBase),
            },
        );
        more = unsafe { Process32NextW(snapshot.0, &mut entry) }.is_ok();
    }
    out
}

pub(crate) fn handle_count(pid: u32) -> Option<u32> {
    let process = open_process(pid, PROCESS_QUERY_LIMITED_INFORMATION).ok()?;
    let mut count = 0;
    unsafe { GetProcessHandleCount(process.0, &mut count) }.ok()?;
    Some(count)
}
