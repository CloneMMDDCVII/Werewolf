use crate::fixture::GameFixture;
use game_engine::{evaluate_winner_with_kills, KillEvent, PlayerState, WinOutcome};
use shared::Team;

#[derive(Debug)]
pub enum ReplayResult {
    Match,
    Mismatch { expected: shared::Team, got: shared::Team },
    NotEvaluated(&'static str),
}

/// Replays a single fixture's final player states through the win-condition
/// evaluator and compares against the recorded winner.
pub fn replay(fixture: &GameFixture) -> ReplayResult {
    let expected = fixture.winner.as_team();

    // We don't have InLove data in the fixture export, so Lovers outcomes
    // can never be evaluated by this engine yet.
    if expected == Team::Lovers {
        return ReplayResult::NotEvaluated("Lovers outcome needs InLove data we don't export");
    }

    // The `role` field is the role a player was *assigned*, not necessarily
    // their role at game-end: Thief/Doppelganger copy another role on death,
    // WildChild/Traitor/SnowWolf can transform into Wolf mid-game. Our SQL
    // export doesn't capture the transformation, so any game involving one
    // of these roles can't be reliably replayed from role data alone.
    if fixture.players.iter().any(|p| {
        matches!(
            p.role,
            shared::Role::Thief
                | shared::Role::Doppelganger
                | shared::Role::WildChild
                | shared::Role::Traitor
                | shared::Role::SnowWolf
        )
    }) {
        return ReplayResult::NotEvaluated(
            "game includes a transformable role (Thief/Doppelganger/WildChild/Traitor/SnowWolf) \
             whose final role isn't captured by the export",
        );
    }

    let players: Vec<PlayerState> = fixture
        .players
        .iter()
        .map(|p| PlayerState {
            id: p.telegram_id as u64,
            role: p.role,
            alive: p.survived,
        })
        .collect();

    let kills: Vec<KillEvent> = fixture
        .kills
        .iter()
        .map(|k| {
            let victim_role = fixture
                .players
                .iter()
                .find(|p| p.telegram_id == k.victim_telegram_id)
                .map(|p| p.role)
                .expect("kill victim should be a player in the game");
            KillEvent {
                victim_role,
                method: k.method,
            }
        })
        .collect();

    match evaluate_winner_with_kills(&players, &kills) {
        WinOutcome::Team(got) if got == expected => ReplayResult::Match,
        WinOutcome::Team(got) => ReplayResult::Mismatch { expected, got },
        WinOutcome::RngDependent(reason)
        | WinOutcome::InsufficientData(reason)
        | WinOutcome::Unimplemented(reason) => ReplayResult::NotEvaluated(reason),
    }
}
