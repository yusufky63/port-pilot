//! Job Objects for managed runs. Every process of a run, including the ones it starts later,
//! lives in one job, so the run can be listed, signalled and, with the user's consent, ended as
//! a whole.
//!
//! A run is started suspended, put into its job and only then resumed: a process that ran
//! even briefly outside the job could start children that escape it.

use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{ERROR_MORE_DATA, HANDLE};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    JOBOBJECT_BASIC_PROCESS_ID_LIST, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectBasicAccountingInformation, JobObjectBasicProcessIdList,
    JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
    TerminateJobObject,
};
use windows::Win32::System::Threading::{
    CREATE_SUSPENDED, OpenThread, ResumeThread, THREAD_SUSPEND_RESUME,
};
use windows::core::{Owned, PCWSTR};

/// An unnamed job owned by this process.
pub struct Job {
    handle: Owned<HANDLE>,
}

// A job handle is a kernel handle; the calls below are safe from any thread.
unsafe impl Send for Job {}
unsafe impl Sync for Job {}

impl Job {
    /// With `kill_on_close`, closing the last handle (this process exiting or crashing) ends
    /// every process in the job, so a run never outlives the program that owns it.
    pub fn new(kill_on_close: bool) -> Result<Job, String> {
        let handle = unsafe { CreateJobObjectW(None, PCWSTR::null()) }
            .map_err(|e| format!("could not create a job object: {e}"))?;
        let job = Job {
            handle: unsafe { Owned::new(handle) },
        };
        job.set_kill_on_close(kill_on_close)?;
        Ok(job)
    }

    pub fn set_kill_on_close(&self, on: bool) -> Result<(), String> {
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = if on {
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        } else {
            JOB_OBJECT_LIMIT(0)
        };
        unsafe {
            SetInformationJobObject(
                *self.handle,
                JobObjectExtendedLimitInformation,
                (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        }
        .map_err(|e| format!("could not set the job's limits: {e}"))
    }

    pub fn assign(&self, process: HANDLE) -> Result<(), String> {
        unsafe { AssignProcessToJobObject(*self.handle, process) }
            .map_err(|e| format!("could not put the process into its job: {e}"))
    }

    /// Processes in the job that have not exited.
    pub fn active_processes(&self) -> u32 {
        let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        let ok = unsafe {
            QueryInformationJobObject(
                Some(*self.handle),
                JobObjectBasicAccountingInformation,
                (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                None,
            )
        };
        if ok.is_ok() { info.ActiveProcesses } else { 0 }
    }

    /// PIDs of the processes in the job right now.
    pub fn process_ids(&self) -> Vec<u32> {
        let header = size_of::<JOBOBJECT_BASIC_PROCESS_ID_LIST>() - size_of::<usize>();
        let mut capacity = 64usize;
        loop {
            // usize storage keeps the list aligned for its usize entries.
            let words = (header + capacity * size_of::<usize>()).div_ceil(size_of::<usize>());
            let mut buf = vec![0usize; words];
            let result = unsafe {
                QueryInformationJobObject(
                    Some(*self.handle),
                    JobObjectBasicProcessIdList,
                    buf.as_mut_ptr().cast(),
                    (buf.len() * size_of::<usize>()) as u32,
                    None,
                )
            };
            let list = unsafe { &*(buf.as_ptr() as *const JOBOBJECT_BASIC_PROCESS_ID_LIST) };
            match result {
                Ok(()) => {
                    let n = list.NumberOfProcessIdsInList as usize;
                    let ids = unsafe { std::slice::from_raw_parts(list.ProcessIdList.as_ptr(), n) };
                    return ids.iter().map(|p| *p as u32).collect();
                }
                Err(e) if e.code() == ERROR_MORE_DATA.to_hresult() && capacity < 65_536 => {
                    capacity = (list.NumberOfAssignedProcesses as usize).max(capacity * 2);
                }
                Err(_) => return Vec::new(),
            }
        }
    }

    /// Ends every process in the job at once. Only with the user's consent.
    pub fn terminate(&self, exit_code: u32) -> Result<(), String> {
        unsafe { TerminateJobObject(*self.handle, exit_code) }
            .map_err(|e| format!("could not end the job: {e}"))
    }

    /// Waits until no process is left in the job; returns whether that happened in time.
    pub fn wait_empty(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if self.active_processes() == 0 {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// Lets a process created with `CREATE_SUSPENDED` run by resuming its threads (a new process has
/// exactly one).
fn resume_process(pid: u32) -> Result<(), String> {
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }
        .map_err(|e| format!("thread snapshot failed: {e}"))?;
    let snap = unsafe { Owned::new(snap) };
    let mut entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    let mut resumed = 0;
    if unsafe { Thread32First(*snap, &mut entry) }.is_ok() {
        loop {
            if entry.th32OwnerProcessID == pid
                && let Ok(t) =
                    unsafe { OpenThread(THREAD_SUSPEND_RESUME, false, entry.th32ThreadID) }
            {
                let t = unsafe { Owned::new(t) };
                if unsafe { ResumeThread(*t) } != u32::MAX {
                    resumed += 1;
                }
            }
            if unsafe { Thread32Next(*snap, &mut entry) }.is_err() {
                break;
            }
        }
    }
    if resumed == 0 {
        return Err(format!("could not resume process {pid}"));
    }
    Ok(())
}

/// Starts `cmd` inside `job`. `flags` are added to `CREATE_SUSPENDED`; the process is resumed
/// once it is in the job. On failure the half-started process is ended.
pub fn spawn_in_job(cmd: &mut Command, flags: u32, job: &Job) -> std::io::Result<Child> {
    cmd.creation_flags(flags | CREATE_SUSPENDED.0);
    let mut child = cmd.spawn()?;
    let handle = HANDLE(child.as_raw_handle());
    let started = job.assign(handle).and_then(|()| resume_process(child.id()));
    if let Err(e) = started {
        let _ = child.kill();
        let _ = child.wait();
        return Err(std::io::Error::other(e));
    }
    Ok(child)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    fn ping(job: &Job) -> Child {
        let mut cmd = Command::new("ping.exe");
        cmd.args(["-n", "30", "127.0.0.1"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        spawn_in_job(&mut cmd, CREATE_NO_WINDOW, job).expect("spawn ping")
    }

    #[test]
    fn a_spawned_process_runs_inside_its_job() {
        let job = Job::new(true).unwrap();
        let mut child = ping(&job);
        assert_eq!(job.process_ids(), vec![child.id()]);
        assert_eq!(job.active_processes(), 1);
        // It was resumed: it is running, not stuck suspended.
        std::thread::sleep(Duration::from_millis(300));
        assert!(child.try_wait().unwrap().is_none());
        job.terminate(1).unwrap();
        assert!(job.wait_empty(Duration::from_secs(5)));
        assert_eq!(child.wait().unwrap().code(), Some(1));
    }

    #[test]
    fn closing_the_job_ends_its_processes_only_with_kill_on_close() {
        let job = Job::new(true).unwrap();
        let mut child = ping(&job);
        drop(job);
        let deadline = Instant::now() + Duration::from_secs(5);
        while child.try_wait().unwrap().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            child.try_wait().unwrap().is_some(),
            "kill-on-close ended it"
        );

        let job = Job::new(true).unwrap();
        let mut child = ping(&job);
        job.set_kill_on_close(false).unwrap();
        drop(job);
        std::thread::sleep(Duration::from_millis(300));
        assert!(
            child.try_wait().unwrap().is_none(),
            "released jobs keep running"
        );
        let _ = child.kill();
    }
}
