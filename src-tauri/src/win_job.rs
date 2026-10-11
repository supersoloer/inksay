use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE};

pub struct JobHandle(isize);

impl JobHandle {
    pub fn attach(pid: u32) -> Result<Self, String> {
        let job =
            unsafe { CreateJobObjectW(None, PCWSTR::null()) }.map_err(|error| error.to_string())?;
        let guard = Self(job.0 as isize);
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        unsafe {
            SetInformationJobObject(
                guard.raw(),
                JobObjectExtendedLimitInformation,
                &limits as *const _ as _,
                std::mem::size_of_val(&limits) as u32,
            )
        }
        .map_err(|error| error.to_string())?;
        let process = unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, false, pid) }
            .map_err(|error| error.to_string())?;
        let assigned = unsafe { AssignProcessToJobObject(guard.raw(), process) };
        let _ = unsafe { CloseHandle(process) };
        assigned.map_err(|error| error.to_string())?;
        Ok(guard)
    }

    fn raw(&self) -> HANDLE {
        HANDLE(self.0 as _)
    }
}

impl Drop for JobHandle {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.raw()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::time::Duration;

    #[test]
    fn closing_job_terminates_assigned_process() {
        let mut child = Command::new("powershell")
            .args(["-NoProfile", "-Command", "Start-Sleep -Seconds 30"])
            .spawn()
            .unwrap();
        assert!(child.try_wait().unwrap().is_none());
        let job = JobHandle::attach(child.id()).unwrap();
        drop(job);
        for _ in 0..40 {
            if child.try_wait().unwrap().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = child.kill();
        panic!("assigned process survived job close");
    }
}
