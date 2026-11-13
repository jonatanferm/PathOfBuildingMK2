//! Small statistics over samples: percentiles, median, MAD, and a Mann-Whitney U test for the
//! `compare` command's significance hint. Owner: PROF1.

/// The `p`-quantile (0..=1) by nearest rank on the sorted samples (`NaN` when empty).
pub fn quantile(v: &[f64], p: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    let i = ((s.len() - 1) as f64 * p).round() as usize;
    s[i.min(s.len() - 1)]
}

/// The median (mean of the two middle samples for an even count).
pub fn median(v: &[f64]) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    let n = s.len();
    if n % 2 == 1 {
        s[n / 2]
    } else {
        (s[n / 2 - 1] + s[n / 2]) / 2.0
    }
}

/// The median absolute deviation (unscaled).
pub fn mad(v: &[f64]) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    let m = median(v);
    let d: Vec<f64> = v.iter().map(|x| (x - m).abs()).collect();
    median(&d)
}

/// Summary of one metric over the measured repetitions.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Summary {
    pub median: f64,
    /// Median absolute deviation.
    pub mad: f64,
    /// `mad / median` (0 when the median is 0).
    pub rel_mad: f64,
    pub min: f64,
    pub max: f64,
    pub n: usize,
    /// The samples, in repetition order (the `compare` test needs them).
    pub samples: Vec<f64>,
}

impl Summary {
    pub fn of(samples: &[f64]) -> Summary {
        let med = median(samples);
        let m = mad(samples);
        Summary {
            median: med,
            mad: m,
            rel_mad: if med.abs() > 0.0 { m / med.abs() } else { 0.0 },
            min: samples.iter().copied().fold(f64::INFINITY, f64::min),
            max: samples.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            n: samples.len(),
            samples: samples.to_vec(),
        }
    }
}

/// The standard normal CDF (Abramowitz-Stegun 7.1.26 erf; |error| < 1.5e-7).
fn norm_cdf(z: f64) -> f64 {
    let x = z.abs() / std::f64::consts::SQRT_2;
    let t = 1.0 / (1.0 + 0.327_591_1 * x);
    let y = 1.0
        - (((((1.061_405_429 * t - 1.453_152_027) * t) + 1.421_413_741) * t - 0.284_496_736) * t
            + 0.254_829_592)
            * t
            * (-x * x).exp();
    if z >= 0.0 {
        0.5 * (1.0 + y)
    } else {
        0.5 * (1.0 - y)
    }
}

/// Two-sided Mann-Whitney U test of `a` against `b`: the p-value (normal approximation with the
/// tie correction and a continuity correction; exact enough for the hint it gives from n = 4).
/// Identical constant samples give 1; two different constants give 0.
pub fn mann_whitney_p(a: &[f64], b: &[f64]) -> f64 {
    let (n1, n2) = (a.len(), b.len());
    if n1 == 0 || n2 == 0 {
        return 1.0;
    }
    let mut all: Vec<(f64, usize)> = a
        .iter()
        .map(|&x| (x, 0))
        .chain(b.iter().map(|&x| (x, 1)))
        .collect();
    all.sort_by(|x, y| x.0.total_cmp(&y.0));
    // average ranks over ties
    let n = all.len();
    let mut ranks = vec![0.0; n];
    let mut tie_term = 0.0;
    let mut i = 0;
    while i < n {
        let mut j = i;
        while j + 1 < n && all[j + 1].0 == all[i].0 {
            j += 1;
        }
        let r = (i + j) as f64 / 2.0 + 1.0;
        for k in ranks.iter_mut().take(j + 1).skip(i) {
            *k = r;
        }
        let t = (j - i + 1) as f64;
        tie_term += t * t * t - t;
        i = j + 1;
    }
    let r1: f64 = all
        .iter()
        .zip(&ranks)
        .filter(|(x, _)| x.1 == 0)
        .map(|(_, r)| *r)
        .sum();
    let (n1f, n2f) = (n1 as f64, n2 as f64);
    let u1 = r1 - n1f * (n1f + 1.0) / 2.0;
    let mu = n1f * n2f / 2.0;
    let nf = n as f64;
    let var = n1f * n2f / 12.0 * ((nf + 1.0) - tie_term / (nf * (nf - 1.0)));
    if var <= 0.0 {
        // every sample is equal
        return if a == b || (median(a) - median(b)).abs() == 0.0 {
            1.0
        } else {
            0.0
        };
    }
    let z = ((u1 - mu).abs() - 0.5).max(0.0) / var.sqrt();
    (2.0 * (1.0 - norm_cdf(z))).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantiles() {
        let v = [5.0, 1.0, 3.0, 2.0, 4.0];
        assert_eq!(quantile(&v, 0.5), 3.0);
        assert_eq!(quantile(&v, 0.0), 1.0);
        assert_eq!(quantile(&v, 1.0), 5.0);
        assert_eq!(median(&[1.0, 2.0, 3.0, 4.0]), 2.5);
        assert_eq!(mad(&[1.0, 2.0, 3.0, 4.0, 100.0]), 1.0);
        assert!(quantile(&[], 0.5).is_nan());
    }

    #[test]
    fn mann_whitney() {
        let a = [10.0, 11.0, 10.5, 10.2, 10.8, 10.1];
        let b = [12.0, 12.5, 12.2, 12.9, 12.1, 12.4];
        assert!(mann_whitney_p(&a, &b) < 0.01);
        let c = [10.3, 10.6, 10.4, 10.9, 10.0, 11.1];
        assert!(mann_whitney_p(&a, &c) > 0.2);
        assert_eq!(mann_whitney_p(&[3.0; 5], &[3.0; 5]), 1.0);
        assert!(mann_whitney_p(&[3.0; 5], &[4.0; 5]) < 0.01);
        // symmetric
        let p1 = mann_whitney_p(&a, &c);
        let p2 = mann_whitney_p(&c, &a);
        assert!((p1 - p2).abs() < 1e-12);
    }

    #[test]
    fn normal_cdf() {
        assert!((norm_cdf(0.0) - 0.5).abs() < 1e-7);
        assert!((norm_cdf(1.959_964) - 0.975).abs() < 1e-6);
        assert!((norm_cdf(-1.959_964) - 0.025).abs() < 1e-6);
    }
}
