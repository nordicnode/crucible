//! The learned commander as a [`Bot`]: a genome evaluated through the feature
//! extractor + network + decision layer. It observes only [`FeatureInput`],
//! so its behavior is fog-legal by construction.

use crucible_sim::{Command, Game, Player};

use crate::bot::Bot;
use crate::decision::decide;
use crate::features::FeatureInput;

/// A genome playing as a commander.
pub struct GenomeBot {
    pub genome: Vec<f32>,
}

impl GenomeBot {
    pub fn new(genome: Vec<f32>) -> Self {
        GenomeBot { genome }
    }
}

impl Bot for GenomeBot {
    fn name(&self) -> &'static str {
        "genome"
    }

    fn decide(&mut self, game: &Game, player: Player) -> Vec<Command> {
        let input = FeatureInput::from_game(game, player);
        decide(game, player, &self.genome, &input)
    }
}
