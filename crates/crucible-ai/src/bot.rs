//! The [`Bot`] interface: any deterministic policy that issues commands on the
//! command tick. Used by the scripted baselines now and by the learned
//! commander later — the same interface the headless runner drives.

use crucible_sim::{Command, Game, Player};

/// A deterministic match policy.
///
/// Implementors are called once per command tick (every 20 sim ticks) and
/// return zero or more commands. Commands are validated and APM-budgeted by
/// the sim — a bot cannot bypass either.
pub trait Bot: Send {
    /// Human-readable name.
    fn name(&self) -> &'static str;

    /// Produce commands for `player` given the current state.
    ///
    /// *Baseline scripted bots may consult the full `Game` (they are oracle
    /// baselines — see `CONTRACT.md` §5). The learned commander must not; its
    /// feature extraction receives only a `FogView`.*
    fn decide(&mut self, game: &Game, player: Player) -> Vec<Command>;
}
