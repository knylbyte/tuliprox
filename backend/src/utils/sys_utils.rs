#[macro_export]
macro_rules! exit {
    ($($arg:tt)*) => {{
        error!($($arg)*);
        std::process::exit(1);
    }};
}
pub use exit;
#[cfg(target_os = "linux")]
use shared::utils::CONSTANTS;

#[cfg(target_os = "linux")]
fn get_memory_usage_linux() -> std::io::Result<u64> {
    use std::fs::File;
    use std::io::{BufReader, Read};

    let path = "/proc/self/status";
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);

    let mut contents = String::new();
    reader.read_to_string(&mut contents)?;

    if let Some(captures) = CONSTANTS.re_memory_usage.captures(&contents) {
        let memory_kb: u64 = captures[1].parse().unwrap();
        Ok(memory_kb * 1024)
    } else {
        Err(std::io::Error::new(std::io::ErrorKind::NotFound, "VmRSS not found"))
    }
}

#[cfg(target_os = "windows")]
fn get_memory_usage_windows() -> Option<u64> {
    use winapi::um::psapi::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use winapi::um::processthreadsapi::GetCurrentProcess;
    use winapi::shared::minwindef::DWORD;

    unsafe {
        let mut counters: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        let process = GetCurrentProcess();
        GetProcessMemoryInfo(process, &mut counters, std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as DWORD);
        Some(counters.WorkingSetSize as u64)
    }
}

#[cfg(target_os = "macos")]
fn get_memory_usage_macos() -> Option<u64> {
    use libc::{getrusage, rusage, RUSAGE_SELF};
    use std::mem::MaybeUninit;

    unsafe {
        let mut info = MaybeUninit::<rusage>::zeroed();
        if getrusage(RUSAGE_SELF, info.as_mut_ptr()) != 0 {
            return None;
        }
        let info = info.assume_init();
        let rss = u64::try_from(info.ru_maxrss).ok()?;
        Some(rss * 1024)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn get_memory_usage_other() -> Option<u64> {
    None
}

pub fn get_memory_usage() -> Option<u64> {
    #[cfg(target_os = "linux")]
    return get_memory_usage_linux().ok();

    #[cfg(target_os = "windows")]
    return get_memory_usage_windows();

    #[cfg(target_os = "macos")]
    return get_memory_usage_macos();

    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    return get_memory_usage_other();
}
