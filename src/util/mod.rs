pub mod math;

/// Java's `Float.toString`: the shortest decimal that reads back as the same
/// float, with ".0" on whole numbers and computerized scientific notation
/// outside 10^-3 .. 10^7.
pub fn java_float(f: f32) -> String {
    if f.is_nan() {
        return "NaN".into();
    }
    if f.is_infinite() {
        return if f > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        };
    }
    if f == 0.0 {
        return if f.is_sign_negative() {
            "-0.0".into()
        } else {
            "0.0".into()
        };
    }
    let a = f.abs();
    if (1e-3..1e7).contains(&a) {
        let s = format!("{f}");
        if s.contains('.') {
            s
        } else {
            s + ".0"
        }
    } else {
        // Rust "1.5e-5" → Java "1.5E-5", "1e7" → "1.0E7"
        let s = format!("{f:e}");
        let (m, e) = s.split_once('e').unwrap_or((&s, "0"));
        let m = if m.contains('.') {
            m.to_string()
        } else {
            format!("{m}.0")
        };
        format!("{m}E{e}")
    }
}

/// Memory of this process for the F3 screen: (working set, commit charge,
/// physical memory of the computer) in bytes.
#[cfg(windows)]
pub fn memory_usage() -> Option<(u64, u64, u64)> {
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    unsafe {
        let mut pmc: PROCESS_MEMORY_COUNTERS_EX = std::mem::zeroed();
        pmc.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
        if GetProcessMemoryInfo(GetCurrentProcess(), &mut pmc as *mut _ as *mut _, pmc.cb) == 0 {
            return None;
        }
        let mut ms: MEMORYSTATUSEX = std::mem::zeroed();
        ms.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        if GlobalMemoryStatusEx(&mut ms) == 0 {
            return None;
        }
        Some((
            pmc.WorkingSetSize as u64,
            pmc.PrivateUsage as u64,
            ms.ullTotalPhys,
        ))
    }
}

#[cfg(not(windows))]
pub fn memory_usage() -> Option<(u64, u64, u64)> {
    None
}

#[cfg(test)]
mod java_float_tests {
    #[test]
    fn like_float_to_string() {
        use super::java_float;
        assert_eq!(java_float(-90.0), "-90.0");
        assert_eq!(java_float(45.5), "45.5");
        assert_eq!(java_float(0.1), "0.1");
        assert_eq!(java_float(1.0e7), "1.0E7");
        assert_eq!(java_float(1.5e-4), "1.5E-4");
        assert_eq!(java_float(0.0), "0.0");
    }
}
