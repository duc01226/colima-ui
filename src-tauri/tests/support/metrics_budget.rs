//! TC021's fixed acceptance arithmetic, shared by native runs and intent tests.
use crate::commands::metrics_collector::TickObservation;

pub fn window(
    count: usize,
    cpu_seconds: f64,
    wall_seconds: f64,
    ticks: &[TickObservation],
) -> Result<(f64, f64), String> {
    let budget = match count {
        20 => 2.0,
        200 => 5.0,
        _ => return Err("Only independent 20/200 runs qualify".into()),
    };
    if !cpu_seconds.is_finite()
        || cpu_seconds < 0.0
        || !wall_seconds.is_finite()
        || wall_seconds < 60.0
    {
        return Err("Missing/invalid full-window CPU observation".into());
    }
    if ticks.len() < 29
        || ticks.iter().any(|tick| {
            tick.failed
                || tick.samples != count
                || !tick.duration_ms.is_finite()
                || tick.duration_ms < 0.0
        })
        || ticks
            .windows(2)
            .any(|pair| pair[1].sequence != pair[0].sequence + 1)
    {
        return Err("Incomplete/failed fleet ticks cannot qualify".into());
    }
    let cpu = cpu_seconds / wall_seconds * 100.0;
    let mut durations: Vec<_> = ticks.iter().map(|tick| tick.duration_ms).collect();
    durations.sort_by(f64::total_cmp);
    let p95 = durations[(durations.len() * 95).div_ceil(100) - 1];
    if cpu >= budget || p95 >= 2000.0 {
        return Err(format!(
            "Budget exceeded: CPU {cpu:.3}% (limit {budget}%), p95 {p95:.3}ms (limit 2000ms)"
        ));
    }
    Ok((cpu, p95))
}

pub fn resident_growth(
    count: usize,
    baseline: &[u64],
    loaded: &[u64],
) -> Result<(u64, u64, u64), String> {
    if ![20, 200].contains(&count)
        || baseline.len() < 60
        || loaded.is_empty()
        || baseline.contains(&0)
        || loaded.contains(&0)
    {
        return Err("Missing required resident samples".into());
    }
    let baseline = *baseline.iter().max().unwrap();
    let peak = *loaded.iter().max().unwrap();
    let growth = peak.saturating_sub(baseline);
    if count == 200 && growth >= 256 * 1048576 {
        return Err("Resident growth exceeds 256 MiB".into());
    }
    Ok((baseline, peak, growth))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ticks(count: usize) -> Vec<TickObservation> {
        (1..=30)
            .map(|sequence| TickObservation {
                sequence,
                duration_ms: 1.0,
                samples: count,
                failed: false,
            })
            .collect()
    }
    #[test]
    fn budgets_are_strict_and_missing_or_partial_work_never_passes() {
        for count in [20, 200] {
            let cpu_limit = if count == 20 { 1.2 } else { 3.0 };
            assert!(window(count, cpu_limit - 0.001, 60.0, &ticks(count)).is_ok());
            assert!(window(count, cpu_limit, 60.0, &ticks(count)).is_err());
            for cpu in [-1.0, f64::NAN] {
                assert!(window(count, cpu, 60.0, &ticks(count)).is_err());
            }
            assert!(window(count, 0.1, 59.9, &ticks(count)).is_err());
            assert!(window(count, 0.1, 60.0, &ticks(count)[..28]).is_err());
            for mutation in 0..4 {
                let mut rows = ticks(count);
                match mutation {
                    0 => rows[2].samples -= 1,
                    1 => rows[2].failed = true,
                    2 => rows[2].sequence += 1,
                    _ => rows[2].duration_ms = f64::NAN,
                }
                assert!(window(count, 0.1, 60.0, &rows).is_err());
            }
            let mut slow = ticks(count);
            slow.iter_mut().for_each(|tick| tick.duration_ms = 2000.0);
            assert!(window(count, 0.1, 60.0, &slow).is_err());
        }
    }
    #[test]
    fn resident_budget_uses_the_prefleet_max_and_loaded_peak_without_warmup_discard() {
        let mut baseline = vec![100; 60];
        baseline[2] = 1000;
        let loaded = [1001, 268436455, 1002];
        assert_eq!(
            resident_growth(200, &baseline, &loaded).unwrap(),
            (1000, 268436455, 268435455)
        );
        assert!(resident_growth(200, &baseline, &[268436456]).is_err());
        assert!(resident_growth(200, &baseline[..59], &loaded).is_err());
        assert!(resident_growth(200, &[0; 60], &loaded).is_err());
        assert!(resident_growth(200, &baseline, &[]).is_err());
    }
}
