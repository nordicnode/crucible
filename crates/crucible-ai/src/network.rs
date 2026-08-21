//! Hand-rolled feed-forward MLP on flat `Vec<f32>` arrays. Forward pass only —
//! CRUCIBLE never backpropagates (evolution strategy mutates weights directly).
//!
//! Layout: FEATURE_DIM -> 48 (tanh) -> 48 (tanh) -> OUTPUT (linear scores).
//! The genome is the flat concatenation `[W1, b1, W2, b2, W3, b3]`.

use crucible_sim::Rng;

use crate::features::FEATURE_DIM;

pub const HIDDEN1: usize = 48;
pub const HIDDEN2: usize = 48;

/// Output head layout (see `decision.rs`).
pub const BUILD_OUT: usize = 8;
pub const TRAIN_OUT: usize = 7;
/// Army-wide actions: attack-move, defend, scout, and focus-fire (snipe).
pub const ARMY_ACTION_OUT: usize = 4;
pub const SECTOR_OUT: usize = 64;
pub const TECH_OUT: usize = 4;
/// Snipe target-type head (used only when the army action is `Snipe`):
/// enemy harvester, refinery, HQ, or factory.
pub const SNIPE_OUT: usize = 4;
pub const OUTPUT: usize =
    BUILD_OUT + TRAIN_OUT + ARMY_ACTION_OUT + SECTOR_OUT + TECH_OUT + SNIPE_OUT;

pub const W1: usize = FEATURE_DIM * HIDDEN1;
pub const B1: usize = HIDDEN1;
pub const W2: usize = HIDDEN1 * HIDDEN2;
pub const B2: usize = HIDDEN2;
pub const W3: usize = HIDDEN2 * OUTPUT;
pub const B3: usize = OUTPUT;

pub const GENOME_LEN: usize = W1 + B1 + W2 + B2 + W3 + B3;

fn uniform(rng: &mut Rng) -> f32 {
    rng.next_u32() as f32 / u32::MAX as f32
}

/// Standard normal via the sum-of-12-uniforms CLT trick: fully deterministic
/// across platforms (no `ln`/`cos`/`sqrt`), good enough for ES mutation.
fn gaussian(rng: &mut Rng) -> f32 {
    let mut s = 0.0f32;
    for _ in 0..12 {
        s += uniform(rng);
    }
    s - 6.0
}

/// Xavier-uniform init over `[-b, b]` with `b = sqrt(6 / (fan_in + fan_out))`.
fn xavier_bound(fan_in: usize, fan_out: usize) -> f32 {
    (6.0 / (fan_in + fan_out) as f32).sqrt()
}

/// Initialize a random genome.
pub fn init(rng: &mut Rng) -> Vec<f32> {
    let mut g = vec![0.0f32; GENOME_LEN];
    init_layer(rng, &mut g[0..W1], FEATURE_DIM, HIDDEN1);
    init_layer(rng, &mut g[W1 + B1..W1 + B1 + W2], HIDDEN1, HIDDEN2);
    init_layer(rng, &mut g[W1 + B1 + W2 + B2..], HIDDEN2, OUTPUT);
    // Biases start at zero.
    g
}

fn init_layer(rng: &mut Rng, w: &mut [f32], fan_in: usize, fan_out: usize) {
    let b = xavier_bound(fan_in, fan_out);
    for v in w.iter_mut() {
        *v = (uniform(rng) * 2.0 - 1.0) * b;
    }
}

/// `out[i] = b[i] + sum_j w[i*n + j] * x[j]`, applying tanh if requested.
fn affine(w: &[f32], b: &[f32], x: &[f32], out: &mut [f32], activate: bool) {
    let m = out.len();
    let n = x.len();
    for i in 0..m {
        let mut acc = b[i];
        for j in 0..n {
            acc += w[i * n + j] * x[j];
        }
        out[i] = if activate { acc.tanh() } else { acc };
    }
}

/// Forward pass. Input length must equal [`FEATURE_DIM`].
pub fn forward(genome: &[f32], input: &[f32]) -> Vec<f32> {
    debug_assert_eq!(genome.len(), GENOME_LEN);
    debug_assert_eq!(input.len(), FEATURE_DIM);

    let (w1, rest) = genome.split_at(W1);
    let (b1, rest) = rest.split_at(B1);
    let (w2, rest) = rest.split_at(W2);
    let (b2, rest) = rest.split_at(B2);
    let (w3, b3) = rest.split_at(W3);

    let mut h1 = vec![0.0f32; HIDDEN1];
    affine(w1, b1, input, &mut h1, true);

    let mut h2 = vec![0.0f32; HIDDEN2];
    affine(w2, b2, &h1, &mut h2, true);

    let mut out = vec![0.0f32; OUTPUT];
    affine(w3, b3, &h2, &mut out, false);
    out
}

/// Gaussian mutation in place. `sigma` is the per-weight standard deviation;
/// `macro_rate` is the probability of re-perturbing each weight at `3*sigma`.
pub fn mutate(rng: &mut Rng, genome: &mut [f32], sigma: f32, macro_rate: f32) {
    for v in genome.iter_mut() {
        let s = if uniform(rng) < macro_rate {
            3.0 * sigma
        } else {
            sigma
        };
        *v += gaussian(rng) * s;
        *v = v.clamp(-8.0, 8.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genome_len_matches_layer_sizes() {
        // FEATURE_DIM is 224 with the plan §5.2 history embedding (2 stacked
        // command ticks): W1 = 224*48, W3 = 48*91 (the snipe head added 4
        // target-type outputs and a 4th army action).
        assert_eq!(GENOME_LEN, 17_611);
        assert_eq!(OUTPUT, 91);
    }

    #[test]
    fn forward_is_deterministic_and_bounded() {
        let mut rng = Rng::from_seed(7);
        let g = init(&mut rng);
        let input = vec![0.5f32; FEATURE_DIM];
        let a = forward(&g, &input);
        let b = forward(&g, &input);
        assert_eq!(a, b);
        assert_eq!(a.len(), OUTPUT);
        for v in &a {
            assert!(v.is_finite());
        }
    }

    #[test]
    fn mutate_changes_weights_and_stays_finite() {
        let mut rng = Rng::from_seed(3);
        let mut g = init(&mut rng);
        let before = g.clone();
        mutate(&mut rng, &mut g, 0.05, 0.1);
        assert_ne!(g, before);
        assert!(g.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn same_seed_same_genome() {
        let mut a = Rng::from_seed(42);
        let mut b = Rng::from_seed(42);
        assert_eq!(init(&mut a), init(&mut b));
    }
}
