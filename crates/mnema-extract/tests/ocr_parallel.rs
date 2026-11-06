//! How many `--ocr` helpers a worker starts for one scanned PDF (D196).

use mnema_extract::ocr_parallel::{MAX_PROCESSES, p_cores, processes};

#[test]
fn performance_cores_decide_when_the_machine_names_them() {
    // M1 (4P), M2 Max (8P), and a part with more P-cores than the measured
    // plateau: capped.
    assert_eq!(processes(Some(4), Some(8), None), 4);
    assert_eq!(processes(Some(8), Some(12), None), 8);
    assert_eq!(processes(Some(12), Some(16), None), MAX_PROCESSES);
    assert_eq!(MAX_PROCESSES, 8, "the plateau measured 2026-11-06");
}

#[test]
fn without_a_p_core_count_half_the_parallelism_is_taken() {
    assert_eq!(processes(None, Some(16), None), 8);
    assert_eq!(processes(None, Some(6), None), 3);
    assert_eq!(processes(None, Some(32), None), MAX_PROCESSES);
    // A zero from sysctl is no answer, not "zero cores".
    assert_eq!(processes(Some(0), Some(6), None), 3);
}

#[test]
fn never_fewer_than_one() {
    assert_eq!(processes(None, Some(1), None), 1);
    assert_eq!(processes(None, None, None), 1);
    assert_eq!(processes(Some(0), Some(0), None), 1);
}

#[test]
fn the_environment_overrides_and_a_bad_value_is_ignored() {
    assert_eq!(processes(Some(8), Some(12), Some("3")), 3);
    assert_eq!(processes(Some(2), Some(4), Some("6")), 6);
    assert_eq!(processes(Some(8), Some(12), Some("1")), 1);
    // Past the cap is the cap; a value that names no count is no override.
    assert_eq!(processes(Some(4), Some(8), Some("100")), MAX_PROCESSES);
    for bad in ["0", "x", "", "-2", "2.5"] {
        assert_eq!(processes(Some(4), Some(8), Some(bad)), 4, "{bad:?}");
    }
}

#[cfg(target_os = "macos")]
#[test]
fn this_mac_names_its_performance_cores() {
    let n = p_cores().expect("hw.perflevel0.physicalcpu exists on Apple silicon");
    let all = std::thread::available_parallelism().unwrap().get();
    assert!((1..=all).contains(&n), "{n} P-cores of {all}");
}

#[cfg(not(target_os = "macos"))]
#[test]
fn elsewhere_there_is_no_p_core_count() {
    assert_eq!(p_cores(), None);
}
