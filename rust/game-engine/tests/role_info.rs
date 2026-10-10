//! features/role-info.feature: what players are told about their role,
//! when roles are dealt and whenever one changes.

use async_trait::async_trait;
use game_engine::messages::{Announcement, Message, MessageArg, Note};
use game_engine::orchestrator::{AlivePlayer, NarrationEvent, Presenter, Prompt};
use game_engine::rng::SeededRngSource;
use game_engine::role_info::{role_info, transform_tells, Cause};
use game_engine::roles::PlayerId;
use game_engine::rules::GameRules;
use game_engine::run_game_with_rules;
use shared::{Role, ALL_ROLES};

const P1: PlayerId = PlayerId(1);
const P2: PlayerId = PlayerId(2);
const P3: PlayerId = PlayerId(3);
const P4: PlayerId = PlayerId(4);

fn roster(roles: &[Role]) -> Vec<AlivePlayer> {
    roles.iter().enumerate().map(|(i, &role)| AlivePlayer { id: PlayerId(i as u64 + 1), role }).collect()
}

fn msg(key: &'static str, args: Vec<MessageArg>) -> Message {
    Message::new(key, args)
}

fn players(ids: &[PlayerId]) -> MessageArg {
    MessageArg::Players(ids.to_vec())
}

fn tell(to: PlayerId, message: Message) -> NarrationEvent {
    NarrationEvent::Tell { to, message }
}

#[test]
fn role_info_matches_get_role_info() {
    let village = roster(&[Role::Villager, Role::Fool, Role::Thief, Role::Beholder]);
    assert_eq!(role_info(Role::Villager, &village, false), msg("RoleInfoVillager", vec![]));
    assert_eq!(role_info(Role::Fool, &village, false), msg("RoleInfoSeer", vec![]));
    assert_eq!(role_info(Role::Thief, &village, false), msg("RoleInfoThief", vec![]));
    assert_eq!(role_info(Role::Thief, &village, true), msg("RoleInfoThiefFull", vec![]));
    assert_eq!(
        role_info(Role::Beholder, &village, false),
        msg("RoleInfoBeholder", vec![]).followed_by(msg("NoSeer", vec![]))
    );
    let with_seer = roster(&[Role::Beholder, Role::Seer]);
    assert_eq!(
        role_info(Role::Beholder, &with_seer, false),
        msg("RoleInfoBeholder", vec![]).followed_by(msg("BeholderSeer", vec![MessageArg::Player(P2)]))
    );
}

#[test]
fn teams_are_listed_when_there_is_more_than_one() {
    let lone = roster(&[Role::Mason, Role::Cultist, Role::Wolf, Role::Villager]);
    assert_eq!(role_info(Role::Mason, &lone, false), msg("RoleInfoMason", vec![]));
    assert_eq!(role_info(Role::Cultist, &lone, false), msg("RoleInfoCultist", vec![]));
    assert_eq!(role_info(Role::Wolf, &lone, false), msg("RoleInfoWolf", vec![]));

    let teams = roster(&[Role::Mason, Role::Mason, Role::Cultist, Role::Cultist, Role::Wolf, Role::AlphaWolf]);
    assert_eq!(
        role_info(Role::Mason, &teams, false),
        msg("RoleInfoMason", vec![]).followed_by(msg("MasonTeam", vec![players(&[P1, P2])]))
    );
    assert_eq!(
        role_info(Role::Cultist, &teams, false),
        msg("RoleInfoCultist", vec![]).followed_by(msg("CultistsList", vec![players(&[P3, P4])]))
    );
    assert_eq!(
        role_info(Role::AlphaWolf, &teams, false),
        msg("RoleInfoAlphaWolf", vec![]).followed_by(msg("WolfPackList", vec![players(&[PlayerId(5), PlayerId(6)])]))
    );

    let snow = roster(&[Role::Wolf, Role::SnowWolf, Role::Wolf]);
    assert_eq!(
        role_info(Role::SnowWolf, &snow, false),
        msg("RoleInfoSnowWolf", vec![])
            .followed_by(msg("WolfPackSnowList", vec![players(&[P1, P3]), MessageArg::Player(P2)]))
    );
}

#[test]
fn the_cursed_learns_the_pack_and_the_pack_learns_of_them() {
    // P3, the Cursed, has already turned.
    let alive = roster(&[Role::Wolf, Role::SnowWolf, Role::Wolf]);
    let told = transform_tells(P3, Role::Cursed, Role::Wolf, Cause::BiteCursed, &alive, false);
    assert_eq!(
        told,
        vec![
            tell(P3, msg("CursedBitten", vec![]).followed_by(msg("WolfPackSnowList", vec![players(&[P1]), MessageArg::Player(P2)]))),
            tell(P1, msg("CursedBittenToWolves", vec![MessageArg::Player(P3)])),
            tell(P2, msg("CursedBittenToWolves", vec![MessageArg::Player(P3)])),
        ]
    );
}

#[test]
fn a_bitten_cultist_is_missed_by_the_cult() {
    let alive = roster(&[Role::Wolf, Role::Cultist, Role::Wolf]);
    let told = transform_tells(P3, Role::Cultist, Role::Wolf, Cause::AlphaBitten, &alive, false);
    assert_eq!(
        told,
        vec![
            tell(P2, msg("CultistBitten", vec![MessageArg::Player(P3)])),
            tell(P3, msg("BittenTurned", vec![]).followed_by(msg("WolfPackList", vec![players(&[P1])]))),
        ]
    );
}

#[test]
fn a_convert_meets_the_cult_and_leaves_the_masons() {
    let alive = roster(&[Role::Cultist, Role::Mason, Role::Cultist]);
    let told = transform_tells(P3, Role::Mason, Role::Cultist, Cause::ConvertToCult, &alive, false);
    assert_eq!(
        told,
        vec![
            tell(P2, msg("MasonConverted", vec![MessageArg::Player(P3)])),
            tell(P3, msg("CultConvertYou", vec![])),
            tell(P3, msg("CultTeam", vec![players(&[P1])])),
            tell(
                P1,
                msg("CultJoin", vec![MessageArg::Player(P3)]).followed_by(msg("CultistsList", vec![players(&[P1, P3])]))
            ),
        ]
    );
}

#[test]
fn the_apprentice_and_the_beholder_hear_of_the_new_seer() {
    let alive = roster(&[Role::Seer, Role::Beholder]);
    let told = transform_tells(P1, Role::ApprenticeSeer, Role::Seer, Cause::ApprenticeSeer { model: Some(P4) }, &alive, false);
    assert_eq!(
        told,
        vec![
            tell(P1, msg("ApprenticeNowSeer", vec![MessageArg::Player(P4)])),
            tell(P2, msg("BeholderNewSeer", vec![MessageArg::Player(P1), MessageArg::Player(P4)])),
        ]
    );
}

#[test]
fn a_doppelganger_who_becomes_a_wolf_meets_the_pack() {
    let alive = roster(&[Role::Wolf, Role::SnowWolf, Role::AlphaWolf]);
    let told = transform_tells(P3, Role::Doppelganger, Role::AlphaWolf, Cause::Doppelganger { model: P4 }, &alive, false);
    assert_eq!(
        told,
        vec![
            tell(P1, msg("DGToWolf", vec![MessageArg::Player(P3)])),
            tell(P2, msg("DGToWolf", vec![MessageArg::Player(P3)])),
            tell(
                P3,
                msg("DGTransformToAlphaWithSnow", vec![MessageArg::Player(P4), players(&[P1]), MessageArg::Player(P2)])
            ),
        ]
    );
}

#[test]
fn a_doppelganger_with_no_team_is_told_the_new_role() {
    let alive = roster(&[Role::Gunner, Role::Villager]);
    let told = transform_tells(P1, Role::Doppelganger, Role::Gunner, Cause::Doppelganger { model: P4 }, &alive, false);
    assert_eq!(told, vec![tell(P1, msg("RoleInfoGunner", vec![]))]);
}

#[test]
fn a_thief_who_steals_into_the_cult_is_introduced() {
    let alive = roster(&[Role::Cultist, Role::Villager, Role::Cultist]);
    let told = transform_tells(P3, Role::Thief, Role::Cultist, Cause::ThiefSteal { from: P2 }, &alive, false);
    assert_eq!(
        told,
        vec![
            tell(P3, msg("RoleInfoCultist", vec![]).followed_by(msg("CultistsList", vec![players(&[P1, P3])]))),
            tell(P1, msg("ThiefToCult", vec![MessageArg::Player(P2), MessageArg::Player(P3)])),
        ]
    );
}

#[test]
fn the_last_converts_are_told_nothing() {
    let alive = roster(&[Role::Cultist, Role::Cultist]);
    assert_eq!(transform_tells(P2, Role::Villager, Role::Cultist, Cause::AutoConvertToCult, &alive, false), vec![]);
}

/// Answers prompts from a table, says no to toggles, logs everything.
struct Game {
    answers: Vec<(Prompt, PlayerId)>,
    round: u32,
    asked: Vec<(PlayerId, Prompt, u32)>,
    narrated: Vec<NarrationEvent>,
}

impl Game {
    fn new(answers: &[(Prompt, PlayerId)]) -> Self {
        Game { answers: answers.to_vec(), round: 1, asked: vec![], narrated: vec![] }
    }

    fn told(&self, to: PlayerId) -> Vec<&'static str> {
        self.narrated
            .iter()
            .filter_map(|e| match e {
                NarrationEvent::Tell { to: t, message } if *t == to => Some(message.key),
                NarrationEvent::Private { to: t, note } if *t == to => Some(note.locale_key()),
                _ => None,
            })
            .collect()
    }
}

#[async_trait(?Send)]
impl Presenter for Game {
    async fn ask_targets(&mut self, player: PlayerId, prompt: Prompt, options: &[PlayerId], _: usize) -> Option<Vec<PlayerId>> {
        self.asked.push((player, prompt, self.round));
        let answer = self.answers.iter().find(|(p, _)| *p == prompt).map(|&(_, t)| t)?;
        options.contains(&answer).then(|| vec![answer])
    }

    async fn ask_toggle(&mut self, _: PlayerId, _: Prompt) -> bool {
        false
    }

    fn advance_round(&mut self) {
        self.round += 1;
    }

    async fn narrate(&mut self, event: NarrationEvent) {
        self.narrated.push(event);
    }
}

async fn play(roles: &[Role], presenter: &mut Game, rounds: u32) {
    let rules = GameRules::default();
    run_game_with_rules(&roster(roles), ALL_ROLES, &rules, presenter, &mut SeededRngSource::new(0), rounds).await;
}

#[tokio::test]
async fn everyone_is_told_their_role_before_anything_else() {
    let roles = [Role::Wolf, Role::Wolf, Role::Seer, Role::Villager, Role::Villager, Role::Villager];
    let mut game = Game::new(&[]);
    play(&roles, &mut game, 1).await;
    let first: Vec<&NarrationEvent> = game.narrated.iter().take(roles.len()).collect();
    for (i, role) in roles.iter().enumerate() {
        let id = PlayerId(i as u64 + 1);
        let NarrationEvent::Tell { to, message } = first[i] else { panic!("{:?}", first[i]) };
        assert_eq!(*to, id);
        assert_eq!(*message, role_info(*role, &roster(&roles), false));
    }
}

#[tokio::test]
async fn role_models_are_picked_on_the_first_night_only() {
    let roles = [Role::WildChild, Role::Doppelganger, Role::Wolf, Role::Villager, Role::Villager, Role::Villager, Role::Villager];
    let mut game = Game::new(&[(Prompt::WildChildRoleModel, P4)]);
    play(&roles, &mut game, 2).await;
    let asked = |player, prompt| game.asked.iter().filter(|a| a.0 == player && a.1 == prompt).map(|a| a.2).collect::<Vec<_>>();
    assert_eq!(asked(P1, Prompt::WildChildRoleModel), vec![1]);
    assert_eq!(asked(P2, Prompt::DoppelgangerRoleModel), vec![1]);
    // The Doppelganger didn't answer, so one was picked for them.
    assert!(game.told(P2).contains(&"RoleModelChosen"));
    assert!(!game.told(P1).contains(&"RoleModelChosen"));
}

#[tokio::test]
async fn a_random_role_model_is_someone_else() {
    for seed in 0..20 {
        let roles = [Role::Doppelganger, Role::Wolf, Role::Villager, Role::Villager, Role::Villager];
        let mut game = Game::new(&[]);
        let rules = GameRules::default();
        run_game_with_rules(&roster(&roles), ALL_ROLES, &rules, &mut game, &mut SeededRngSource::new(seed), 1).await;
        let picked = game.narrated.iter().find_map(|e| match e {
            NarrationEvent::Private { to, note: Note::RoleModelChosen { target } } if *to == P1 => Some(*target),
            _ => None,
        });
        assert!(picked.is_some_and(|t| t != P1), "seed {seed}: {picked:?}");
    }
}

#[tokio::test]
async fn the_cursed_turns_and_hears_from_the_pack() {
    let roles = [Role::Wolf, Role::Cursed, Role::Villager, Role::Villager, Role::Villager];
    let mut game = Game::new(&[(Prompt::WolfEat, P2)]);
    play(&roles, &mut game, 1).await;
    assert!(game.narrated.contains(&NarrationEvent::Transform { player: P2, from: Role::Cursed, to: Role::Wolf }));
    assert!(game.told(P2).contains(&"CursedBitten"));
    assert!(game.told(P1).contains(&"CursedBittenToWolves"));
}

#[tokio::test]
async fn shooting_the_wise_elder_is_announced() {
    let roles = [Role::Gunner, Role::WiseElder, Role::Wolf, Role::Villager, Role::Villager, Role::Villager];
    let mut game = Game::new(&[(Prompt::GunnerShoot { bullets: 2 }, P2)]);
    play(&roles, &mut game, 1).await;
    assert!(game.narrated.contains(&NarrationEvent::Announce(Announcement::GunnerShotWiseElder { gunner: P1, elder: P2 })));
    assert!(game.narrated.contains(&NarrationEvent::Transform { player: P1, from: Role::Gunner, to: Role::Villager }));
}
