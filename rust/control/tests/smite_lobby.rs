//! `/smite` while a group is still joining (Werewolf.cs::FleePlayer, the
//! `IsJoining` branch, 5384-5389): the admin's targets come out of
//! `moderation::smite_targets` and leave the lobby through the same
//! `Lobby::leave` that `/flee` uses. Smite stays silent where `/flee`
//! would complain, so every error here is something the caller drops.
use control::lobby::{Candidate, LeaveError, Lobby, LobbyState};
use moderation::{smite_targets, CommandMessage, Mention, PlayerDirectory, PlayerRecord, ReplyTo};

struct Nobody;

impl PlayerDirectory for Nobody {
    fn by_id(&self, _: i64) -> Option<PlayerRecord> {
        None
    }
    fn by_username(&self, _: &str) -> Option<PlayerRecord> {
        None
    }
}

fn lobby_with(ids: &[i64]) -> Lobby {
    let mut lobby = Lobby::with_limits(1, 35);
    for id in ids {
        lobby
            .join(Candidate {
                telegram_id: *id,
                name: format!("P{id}"),
            })
            .unwrap();
    }
    lobby
}

fn smite(lobby: &mut Lobby, message: &CommandMessage) -> Vec<Result<(), LeaveError>> {
    smite_targets(message, &Nobody)
        .into_iter()
        .map(|id| lobby.leave(id))
        .collect()
}

#[test]
fn smiting_a_joined_player_removes_them() {
    let mut lobby = lobby_with(&[10, 20]);
    let message = CommandMessage {
        sender_id: 1,
        reply_to: Some(ReplyTo {
            from: 10,
            forwarded_from: None,
        }),
        ..Default::default()
    };
    assert_eq!(smite(&mut lobby, &message), vec![Ok(())]);
    let left: Vec<i64> = lobby.players().iter().map(|p| p.telegram_id).collect();
    assert_eq!(left, vec![20]);
}

#[test]
fn smiting_several_players_at_once() {
    let mut lobby = lobby_with(&[10, 20, 30]);
    let rita = PlayerRecord {
        telegram_id: 30,
        name: "P30".into(),
        username: None,
    };
    let message = CommandMessage {
        sender_id: 1,
        reply_to: Some(ReplyTo {
            from: 10,
            forwarded_from: None,
        }),
        mentions: vec![Mention::User(rita)],
        argument: Some("P30".into()),
    };
    assert_eq!(smite(&mut lobby, &message), vec![Ok(()), Ok(())]);
    assert_eq!(lobby.players().len(), 1);
}

#[test]
fn smiting_someone_who_never_joined_changes_nothing() {
    let mut lobby = lobby_with(&[10]);
    let message = CommandMessage {
        sender_id: 1,
        argument: Some("99".into()),
        ..Default::default()
    };
    assert_eq!(smite(&mut lobby, &message), vec![Err(LeaveError::NotInGame)]);
    assert_eq!(lobby.players().len(), 1);
}

#[test]
fn once_the_game_has_started_the_lobby_hands_smite_to_the_running_game() {
    let mut lobby = lobby_with(&[10, 20]);
    lobby.force_start();
    lobby.try_start();
    assert_eq!(lobby.state(), LobbyState::Started);
    let message = CommandMessage {
        sender_id: 1,
        argument: Some("10".into()),
        ..Default::default()
    };
    // `NotJoining` tells the caller to send the id to the running game's
    // `drain_fled_players` instead (see game-engine/tests/smite.rs).
    assert_eq!(smite(&mut lobby, &message), vec![Err(LeaveError::NotJoining)]);
}
