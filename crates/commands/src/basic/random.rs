use std::sync::Mutex;

/// Injectable source of uniform random numbers in `[0, 1)`.
pub trait RandomSource: Send + Sync {
    fn next_f64(&self) -> f64;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ThreadRandom;

impl RandomSource for ThreadRandom {
    fn next_f64(&self) -> f64 {
        fastrand::f64()
    }
}

/// A deterministic random source for tests.
#[derive(Debug)]
pub struct SeededRandom {
    inner: Mutex<fastrand::Rng>,
}

impl SeededRandom {
    pub fn new(seed: u64) -> Self {
        Self {
            inner: Mutex::new(fastrand::Rng::with_seed(seed)),
        }
    }
}

impl RandomSource for SeededRandom {
    fn next_f64(&self) -> f64 {
        self.inner.lock().expect("seeded random lock").f64()
    }
}

/// Approximates a multinomial count vector with sequential binomial normal approximations.
///
/// The last bucket receives the remaining trials, so the returned counts always sum to `total`.
pub fn approximate_multinomial(
    total: u64,
    probabilities: &[f64],
    random: &dyn RandomSource,
) -> Vec<u64> {
    assert!(!probabilities.is_empty(), "probabilities must not be empty");
    let sum: f64 = probabilities.iter().sum();
    assert!(sum > 0.0, "probability sum must be positive");

    let normalized: Vec<f64> = probabilities.iter().map(|value| value / sum).collect();
    let mut counts = vec![0; normalized.len()];
    let mut remaining = total;
    let mut remaining_probability = 1.0;

    for (index, probability) in normalized
        .iter()
        .take(normalized.len().saturating_sub(1))
        .enumerate()
    {
        if remaining == 0 {
            continue;
        }

        let probability = if remaining_probability > 0.0 {
            probability / remaining_probability
        } else {
            0.0
        };

        if probability <= 0.0 {
            continue;
        }
        if probability >= 1.0 {
            counts[index] = remaining;
            remaining = 0;
            remaining_probability = 0.0;
            continue;
        }

        let mean = remaining as f64 * probability;
        let variance = remaining as f64 * probability * (1.0 - probability);
        let sampled = if variance <= 0.0 {
            mean.round()
        } else {
            mean + variance.sqrt() * normal(random)
        };

        let sampled = if sampled.is_finite() {
            sampled.round().clamp(0.0, remaining as f64) as u64
        } else {
            0
        };

        counts[index] = sampled;
        remaining -= sampled;
        remaining_probability -= normalized[index];
    }

    if let Some(last) = counts.last_mut() {
        *last = remaining;
    }

    counts
}

/// Standard normal sample via Box-Muller.
pub fn normal(random: &dyn RandomSource) -> f64 {
    let u1 = random.next_f64().max(f64::MIN_POSITIVE);
    let u2 = random.next_f64();
    (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_always_sum_to_total() {
        let random = SeededRandom::new(7);
        for total in [0, 1, 2, 10, 1_000, 1_000_000] {
            let coin = approximate_multinomial(total, &[0.5, 0.5, 0.000000000002], &random);
            assert_eq!(coin.iter().sum::<u64>(), total);

            let dice = approximate_multinomial(total, &[1.0; 6], &random);
            assert_eq!(dice.len(), 6);
            assert_eq!(dice.iter().sum::<u64>(), total);
        }
    }
}
