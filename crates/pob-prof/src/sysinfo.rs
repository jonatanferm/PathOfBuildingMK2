//! The machine state a run records (so `compare` can say when two runs are not comparable).
//! Owner: PROF1. No host or user names are recorded.

use std::path::Path;
use std::process::Command;

fn read(p: &str) -> String {
    std::fs::read_to_string(p)
        .unwrap_or_default()
        .trim()
        .to_owned()
}

/// `Cpus_allowed_list` of this process (the taskset pinning).
pub fn cpus_allowed() -> String {
    read("/proc/self/status")
        .lines()
        .find_map(|l| l.strip_prefix("Cpus_allowed_list:"))
        .map(|s| s.trim().to_owned())
        .unwrap_or_default()
}

/// Expand a CPU list (`2-4,7`).
pub fn expand_cpus(list: &str) -> Vec<u32> {
    let mut v = Vec::new();
    for part in list.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        if let Some((a, b)) = part.split_once('-') {
            if let (Ok(a), Ok(b)) = (a.parse::<u32>(), b.parse::<u32>()) {
                v.extend(a..=b);
            }
        } else if let Ok(a) = part.parse::<u32>() {
            v.push(a);
        }
    }
    v
}

/// The scaling governors of the allowed CPUs (`schedutil`, or `performance,schedutil` when mixed).
pub fn governor() -> String {
    let mut g: Vec<String> = expand_cpus(&cpus_allowed())
        .into_iter()
        .map(|c| {
            read(&format!(
                "/sys/devices/system/cpu/cpu{c}/cpufreq/scaling_governor"
            ))
        })
        .filter(|s| !s.is_empty())
        .collect();
    g.sort();
    g.dedup();
    g.join(",")
}

/// The CPU model (`lscpu`'s "Model name", or /proc/cpuinfo).
pub fn cpu_model() -> String {
    if let Ok(o) = Command::new("lscpu").env("LC_ALL", "C").output() {
        let s = String::from_utf8_lossy(&o.stdout);
        let names: Vec<&str> = s
            .lines()
            .filter_map(|l| l.strip_prefix("Model name:"))
            .map(str::trim)
            .collect();
        if !names.is_empty() {
            return names.join(" + ");
        }
    }
    read("/proc/cpuinfo")
        .lines()
        .find_map(|l| {
            l.strip_prefix("model name")
                .and_then(|r| r.split_once(':'))
                .map(|(_, v)| v.trim().to_owned())
        })
        .unwrap_or_default()
}

pub fn kernel() -> String {
    read("/proc/sys/kernel/osrelease")
}

/// The 1/5/15 minute load averages.
pub fn loadavg() -> Vec<f64> {
    read("/proc/loadavg")
        .split_whitespace()
        .take(3)
        .filter_map(|x| x.parse().ok())
        .collect()
}

/// `(commit, dirty)` of the git checkout at `dir`.
pub fn git(dir: &Path) -> (String, bool) {
    let commit = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default();
    let dirty = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["status", "--porcelain", "--untracked-files=no"])
        .output()
        .is_ok_and(|o| !o.stdout.is_empty());
    (commit, dirty)
}

/// UTC now as `YYYY-MM-DDTHH:MM:SSZ`.
pub fn now_utc() -> String {
    let s = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (s.div_euclid(86400), s.rem_euclid(86400));
    // civil from days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_lists() {
        assert_eq!(expand_cpus("2-4,7"), vec![2, 3, 4, 7]);
        assert_eq!(expand_cpus(""), Vec::<u32>::new());
        assert_eq!(expand_cpus("0"), vec![0]);
    }

    #[test]
    fn date_format() {
        let d = now_utc();
        assert_eq!(d.len(), 20, "{d}");
        assert!(d.starts_with("20"), "{d}");
    }
}
