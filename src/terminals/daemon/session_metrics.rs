use crate::shared::daemon_wire::DaemonSessionMetrics;
use std::{
    fs,
    path::Path,
    time::{Duration, Instant},
};

const PROC_STAT_UTIME_INDEX: usize = 11;
const PROC_STAT_STIME_INDEX: usize = 12;
const PROC_STAT_PGRP_INDEX: usize = 2;
const PROC_STAT_STARTTIME_INDEX: usize = 19;

pub(super) struct SessionMetricSampler {
    root_pid: Option<u32>,
    process_group_leader: Option<i32>,
    previous_cpu: Option<CpuSample>,
    previous_network: Option<NetworkSample>,
}

#[derive(Clone, Copy)]
struct CpuSample {
    total_ticks: u64,
    captured_at: Instant,
    root_starttime_ticks: u64,
}

#[derive(Clone, Copy)]
struct NetworkSample {
    rx_bytes: u64,
    tx_bytes: u64,
    captured_at: Instant,
    root_starttime_ticks: u64,
}

#[derive(Clone, Copy)]
struct ProcStatFields {
    process_group_id: i32,
    utime_ticks: u64,
    stime_ticks: u64,
    starttime_ticks: u64,
}

impl SessionMetricSampler {
    pub(super) fn new(root_pid: Option<u32>, process_group_leader: Option<i32>) -> Self {
        Self {
            root_pid,
            process_group_leader,
            previous_cpu: None,
            previous_network: None,
        }
    }

    pub(super) fn sample(&mut self) -> DaemonSessionMetrics {
        #[cfg(target_os = "linux")]
        {
            self.sample_linux()
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = self;
            DaemonSessionMetrics::default()
        }
    }

    #[cfg(target_os = "linux")]
    fn sample_linux(&mut self) -> DaemonSessionMetrics {
        let Some(root_pid) = self.root_pid.filter(|pid| *pid > 0) else {
            self.previous_cpu = None;
            self.previous_network = None;
            return DaemonSessionMetrics::default();
        };
        let Some(root_stat) = read_proc_stat_fields(root_pid) else {
            self.previous_cpu = None;
            self.previous_network = None;
            return DaemonSessionMetrics::default();
        };
        let process_group_leader = self
            .process_group_leader
            .filter(|pgid| *pgid > 0)
            .unwrap_or(root_stat.process_group_id);
        let process_ids = process_group_pids(process_group_leader, root_pid);
        let now = Instant::now();

        let total_ticks = total_cpu_ticks(&process_ids);
        let ram_bytes = total_ram_bytes(&process_ids);
        let cpu_pct_milli = sample_cpu_pct_milli(
            &mut self.previous_cpu,
            total_ticks,
            now,
            root_stat.starttime_ticks,
        );

        let (network_rx_bytes, network_tx_bytes) = estimate_network_totals(&process_ids);
        let (net_rx_bytes_per_sec, net_tx_bytes_per_sec) = sample_network_rates(
            &mut self.previous_network,
            network_rx_bytes,
            network_tx_bytes,
            now,
            root_stat.starttime_ticks,
        );

        DaemonSessionMetrics {
            cpu_pct_milli,
            ram_bytes: Some(ram_bytes),
            net_rx_bytes_per_sec,
            net_tx_bytes_per_sec,
        }
    }
}

#[cfg(target_os = "linux")]
fn sample_cpu_pct_milli(
    previous: &mut Option<CpuSample>,
    total_ticks: u64,
    captured_at: Instant,
    root_starttime_ticks: u64,
) -> Option<u32> {
    let previous_sample = *previous;
    *previous = Some(CpuSample {
        total_ticks,
        captured_at,
        root_starttime_ticks,
    });
    let previous_sample = previous_sample?;
    if previous_sample.root_starttime_ticks != root_starttime_ticks {
        return None;
    }
    let elapsed = captured_at.saturating_duration_since(previous_sample.captured_at);
    if elapsed <= Duration::from_millis(10) {
        return None;
    }
    let delta_ticks = total_ticks.saturating_sub(previous_sample.total_ticks) as f64;
    let delta_secs = elapsed.as_secs_f64();
    let cpu_pct = (delta_ticks / clock_ticks_per_second() as f64 / delta_secs) * 100.0;
    Some((cpu_pct.max(0.0) * 1000.0).round() as u32)
}

#[cfg(target_os = "linux")]
fn sample_network_rates(
    previous: &mut Option<NetworkSample>,
    rx_bytes: u64,
    tx_bytes: u64,
    captured_at: Instant,
    root_starttime_ticks: u64,
) -> (Option<u64>, Option<u64>) {
    let previous_sample = *previous;
    *previous = Some(NetworkSample {
        rx_bytes,
        tx_bytes,
        captured_at,
        root_starttime_ticks,
    });
    let Some(previous_sample) = previous_sample else {
        return (None, None);
    };
    if previous_sample.root_starttime_ticks != root_starttime_ticks {
        return (None, None);
    }
    let elapsed = captured_at.saturating_duration_since(previous_sample.captured_at);
    if elapsed <= Duration::from_millis(10) {
        return (None, None);
    }
    let delta_secs = elapsed.as_secs_f64();
    let rx_rate = ((rx_bytes.saturating_sub(previous_sample.rx_bytes)) as f64 / delta_secs)
        .max(0.0)
        .round() as u64;
    let tx_rate = ((tx_bytes.saturating_sub(previous_sample.tx_bytes)) as f64 / delta_secs)
        .max(0.0)
        .round() as u64;
    (Some(rx_rate), Some(tx_rate))
}

#[cfg(target_os = "linux")]
fn process_group_pids(process_group_leader: i32, root_pid: u32) -> Vec<u32> {
    let mut process_ids = fs::read_dir("/proc")
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(Result::ok))
        .filter_map(|entry| {
            entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u32>().ok())
        })
        .filter(|pid| {
            read_proc_stat_fields(*pid)
                .is_some_and(|fields| fields.process_group_id == process_group_leader)
        })
        .collect::<Vec<_>>();
    if !process_ids.contains(&root_pid) && Path::new(&format!("/proc/{root_pid}")).exists() {
        process_ids.push(root_pid);
    }
    process_ids.sort_unstable();
    process_ids.dedup();
    process_ids
}

#[cfg(target_os = "linux")]
fn total_cpu_ticks(process_ids: &[u32]) -> u64 {
    process_ids
        .iter()
        .filter_map(|pid| read_proc_stat_fields(*pid))
        .map(|fields| fields.utime_ticks.saturating_add(fields.stime_ticks))
        .sum()
}

#[cfg(target_os = "linux")]
fn total_ram_bytes(process_ids: &[u32]) -> u64 {
    process_ids
        .iter()
        .map(|pid| read_proc_rss_pages(*pid).saturating_mul(page_size_bytes()))
        .sum()
}

#[cfg(target_os = "linux")]
fn estimate_network_totals(process_ids: &[u32]) -> (u64, u64) {
    process_ids
        .iter()
        .filter(|pid| process_has_tcp_socket(**pid))
        .fold((0_u64, 0_u64), |(rx_total, tx_total), pid| {
            let Some((rx_bytes, tx_bytes)) = read_process_network_io_estimate(*pid) else {
                return (rx_total, tx_total);
            };
            (
                rx_total.saturating_add(rx_bytes),
                tx_total.saturating_add(tx_bytes),
            )
        })
}

#[cfg(target_os = "linux")]
fn read_proc_stat_fields(pid: u32) -> Option<ProcStatFields> {
    let raw = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let right_paren = raw.rfind(')')?;
    let rest = raw.get(right_paren + 2..)?;
    let fields = rest.split_whitespace().collect::<Vec<_>>();
    Some(ProcStatFields {
        process_group_id: fields.get(PROC_STAT_PGRP_INDEX)?.parse().ok()?,
        utime_ticks: fields.get(PROC_STAT_UTIME_INDEX)?.parse().ok()?,
        stime_ticks: fields.get(PROC_STAT_STIME_INDEX)?.parse().ok()?,
        starttime_ticks: fields.get(PROC_STAT_STARTTIME_INDEX)?.parse().ok()?,
    })
}

#[cfg(target_os = "linux")]
fn read_proc_rss_pages(pid: u32) -> u64 {
    fs::read_to_string(format!("/proc/{pid}/statm"))
        .ok()
        .and_then(|raw| raw.split_whitespace().nth(1)?.parse::<u64>().ok())
        .unwrap_or(0)
}

#[cfg(target_os = "linux")]
fn process_has_tcp_socket(pid: u32) -> bool {
    let tcp_inodes = ["tcp", "tcp6"]
        .into_iter()
        .flat_map(|proto| read_tcp_socket_inodes(pid, proto))
        .collect::<std::collections::BTreeSet<_>>();
    if tcp_inodes.is_empty() {
        return false;
    }
    socket_inodes_for_process(pid)
        .into_iter()
        .any(|inode| tcp_inodes.contains(&inode))
}

#[cfg(target_os = "linux")]
fn read_tcp_socket_inodes(pid: u32, proto: &str) -> Vec<u64> {
    fs::read_to_string(format!("/proc/{pid}/net/{proto}"))
        .ok()
        .map(|raw| {
            raw.lines()
                .skip(1)
                .filter_map(|line| line.split_whitespace().nth(9))
                .filter_map(|inode| inode.parse::<u64>().ok())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

#[cfg(target_os = "linux")]
fn socket_inodes_for_process(pid: u32) -> Vec<u64> {
    fs::read_dir(format!("/proc/{pid}/fd"))
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(Result::ok))
        .filter_map(|entry| fs::read_link(entry.path()).ok())
        .filter_map(|link| link.to_str().and_then(parse_socket_inode_link))
        .collect()
}

#[cfg(target_os = "linux")]
fn parse_socket_inode_link(link: &str) -> Option<u64> {
    link.strip_prefix("socket:[")?
        .strip_suffix(']')?
        .parse()
        .ok()
}

#[cfg(target_os = "linux")]
fn read_process_network_io_estimate(pid: u32) -> Option<(u64, u64)> {
    let raw = fs::read_to_string(format!("/proc/{pid}/io")).ok()?;
    let mut rchar = 0_u64;
    let mut wchar = 0_u64;
    let mut read_bytes = 0_u64;
    let mut write_bytes = 0_u64;
    for line in raw.lines() {
        if let Some(value) = line.strip_prefix("rchar:") {
            rchar = value.trim().parse().ok()?;
        } else if let Some(value) = line.strip_prefix("wchar:") {
            wchar = value.trim().parse().ok()?;
        } else if let Some(value) = line.strip_prefix("read_bytes:") {
            read_bytes = value.trim().parse().ok()?;
        } else if let Some(value) = line.strip_prefix("write_bytes:") {
            write_bytes = value.trim().parse().ok()?;
        }
    }
    Some((
        rchar.saturating_sub(read_bytes),
        wchar.saturating_sub(write_bytes),
    ))
}

#[cfg(target_os = "linux")]
fn clock_ticks_per_second() -> u64 {
    clock_ticks_per_second_impl().unwrap_or(100)
}

#[cfg(target_os = "linux")]
fn page_size_bytes() -> u64 {
    page_size_bytes_impl().unwrap_or(4096)
}

#[cfg(target_os = "linux")]
fn clock_ticks_per_second_impl() -> Option<u64> {
    sysconf_value(SYSCONF_CLK_TCK)
}

#[cfg(target_os = "linux")]
fn page_size_bytes_impl() -> Option<u64> {
    sysconf_value(SYSCONF_PAGE_SIZE)
}

#[cfg(target_os = "linux")]
fn sysconf_value(name: i32) -> Option<u64> {
    // SAFETY: `sysconf` is a pure libc query with no aliasing requirements. Negative results mean
    // the value is unavailable.
    let value = unsafe { sysconf(name) };
    (value > 0).then_some(value as u64)
}

#[cfg(target_os = "linux")]
const SYSCONF_CLK_TCK: i32 = 2;
#[cfg(target_os = "linux")]
const SYSCONF_PAGE_SIZE: i32 = 30;

#[cfg(target_os = "linux")]
unsafe extern "C" {
    fn sysconf(name: i32) -> i64;
}

#[cfg(test)]
mod tests {
    use super::{
        parse_socket_inode_link, sample_cpu_pct_milli, sample_network_rates, CpuSample,
        NetworkSample,
    };
    use std::time::{Duration, Instant};

    #[test]
    fn parse_socket_inode_link_extracts_inode_number() {
        assert_eq!(parse_socket_inode_link("socket:[12345]"), Some(12345));
        assert_eq!(parse_socket_inode_link("pipe:[12345]"), None);
    }

    #[test]
    fn cpu_sampling_uses_delta_between_samples() {
        let start = Instant::now();
        let mut previous = Some(CpuSample {
            total_ticks: 100,
            captured_at: start,
            root_starttime_ticks: 77,
        });
        let cpu_pct_milli =
            sample_cpu_pct_milli(&mut previous, 200, start + Duration::from_secs(1), 77)
                .expect("second sample should compute cpu rate");
        assert!(cpu_pct_milli > 0);
    }

    #[test]
    fn network_sampling_resets_when_root_pid_changes() {
        let start = Instant::now();
        let mut previous = Some(NetworkSample {
            rx_bytes: 100,
            tx_bytes: 200,
            captured_at: start,
            root_starttime_ticks: 11,
        });
        let (rx, tx) =
            sample_network_rates(&mut previous, 150, 260, start + Duration::from_secs(1), 12);
        assert_eq!(rx, None);
        assert_eq!(tx, None);
    }
}
