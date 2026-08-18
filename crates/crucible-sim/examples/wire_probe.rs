//! Dump the exact wire JSON for the commands the server deserializes, so we
//! can compare with what the TS client actually sends.
use crucible_sim::{entity::BuildingType, Command, Player};

fn main() {
    let cmd = Command::PlaceBuilding {
        player: Player::P0,
        btype: BuildingType::Refinery,
        tile: (15, 11),
    };
    println!("{}", serde_json::to_string(&cmd).unwrap());
    // And the ClientMsg-shaped wrapper the server's ws.rs parses.
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "type": "commands",
            "cmds": [cmd],
        }))
        .unwrap()
    );
}
