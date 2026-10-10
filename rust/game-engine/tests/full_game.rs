//! Proves `run_game` actually plays a game to completion across multiple
//! rounds, not just one isolated night or day — the thing every other
//! integration test in this crate has been building toward.

use game_engine::messages::Announcement;
use game_engine::orchestrator::{NarrationEvent, Prompt};
use game_engine::rng::SeededRngSource;
use game_engine::roles::PlayerId;
use game_engine::testing::{roster, Script, When};
use game_engine::{run_game, WinOutcome};
use shared::{KillMethod, Role, Team};

#[tokio::test]
async fn wolves_win_once_they_outnumber_the_village() {
    let wolf = PlayerId(1);
    let victim1 = PlayerId(2);
    let victim2 = PlayerId(3);
    let bystander = PlayerId(4);

    let players = roster(&[
        (wolf, Role::Wolf),
        (victim1, Role::Villager),
        (victim2, Role::Villager),
        (bystander, Role::Villager),
    ]);

    // The wolf eats victim1 every night; the village (mis-)votes to lynch
    // victim2 every day. After night 1 + day 1: wolf, bystander alive (2
    // players, 1 wolf) - not yet a majority. The scripts keep targeting
    // players who are already dead once they're gone, which the night and
    // day questions naturally drop (dead players aren't in the alive roster
    // asked from), so the game converges to wolf vs. bystander and the
    // wolf wins by outnumbering (1 wolf >= 1 other).
    let mut presenter = Script::new()
        .pick(Prompt::WolfEat, victim1)
        .pick(Prompt::LynchVote, victim2);

    let mut rng = SeededRngSource::new(0);
    let outcome = run_game(&players, &mut presenter, &mut rng, 10).await;

    assert_eq!(outcome.winner, WinOutcome::Team(Team::Wolf));
    assert_eq!(
        outcome.rounds_played, 1,
        "one full night+day should be enough to bring it to 1 wolf vs 1 villager"
    );
}

#[tokio::test]
async fn a_lynched_tanner_wins_immediately_even_mid_game() {
    let voter = PlayerId(1);
    let tanner = PlayerId(2);
    let wolf = PlayerId(3);
    let villager = PlayerId(4);

    let players = roster(&[
        (voter, Role::Villager),
        (tanner, Role::Tanner),
        (wolf, Role::Wolf),
        (villager, Role::Villager),
    ]);

    // Nobody scripted to eat, so no night death; the day vote lands on the
    // Tanner, who should win immediately regardless of the wolf still
    // being alive.
    let mut presenter = Script::new().pick(Prompt::LynchVote, tanner);
    let mut rng = SeededRngSource::new(0);
    let outcome = run_game(&players, &mut presenter, &mut rng, 10).await;

    assert_eq!(outcome.winner, WinOutcome::Team(Team::Tanner));
    assert_eq!(outcome.rounds_played, 1);
}

#[tokio::test]
async fn a_lynched_hunter_takes_someone_down_with_them() {
    // The village mistakenly lynches the Hunter, who uses their final
    // shot to take a villager with them - resolve_hunter_shots's "a death
    // can trigger one more question" hook, end to end through run_game.
    let wolf = PlayerId(1);
    let hunter = PlayerId(2);
    let doomed_villager = PlayerId(3);
    let survivor = PlayerId(4);

    let players = roster(&[
        (wolf, Role::Wolf),
        (hunter, Role::Hunter),
        (doomed_villager, Role::Villager),
        (survivor, Role::Villager),
    ]);

    let mut presenter = Script::new()
        .pick(Prompt::LynchVote, hunter)
        .pick(When::asked(Prompt::HunterFinalShotLynched).by(hunter), doomed_villager);
    let mut rng = SeededRngSource::new(0);
    let outcome = run_game(&players, &mut presenter, &mut rng, 10).await;

    // Only wolf and survivor remain (1 wolf >= 1 other), so the wolf wins
    // - and only because the Hunter's shot claimed a *second* death this
    // round, not just the lynch itself.
    assert_eq!(outcome.winner, WinOutcome::Team(Team::Wolf));
    let mut deaths = outcome.deaths.clone();
    deaths.sort_by_key(|(id, _)| id.0);
    assert_eq!(
        deaths,
        vec![(hunter, KillMethod::Lynch), (doomed_villager, KillMethod::HunterShot),]
    );
}

#[tokio::test]
async fn a_wolf_eating_one_lover_kills_the_other_too() {
    // Cupid links LoverX/LoverY night 1; the wolf eats LoverX that same
    // night. LoverY should die too, via KillMethod::LoverDied, with no
    // question asked - resolve_lover_deaths's chain, exercised end to end.
    let wolf = PlayerId(1);
    let cupid = PlayerId(2);
    let lover_x = PlayerId(3);
    let lover_y = PlayerId(4);

    let players = roster(&[
        (wolf, Role::Wolf),
        (cupid, Role::Cupid),
        (lover_x, Role::Villager),
        (lover_y, Role::Villager),
    ]);

    let mut presenter = Script::new()
        .picks(Prompt::CupidLink, vec![lover_x, lover_y])
        .pick(When::asked(Prompt::WolfEat).in_round(1), lover_x);
    let mut rng = SeededRngSource::new(0);
    let outcome = run_game(&players, &mut presenter, &mut rng, 1).await;

    let mut deaths = outcome.deaths.clone();
    deaths.sort_by_key(|(id, _)| id.0);
    assert_eq!(
        deaths,
        vec![(lover_x, KillMethod::Eat), (lover_y, KillMethod::LoverDied)],
        "eating one lover should chain-kill the other, unconditionally"
    );
}

#[tokio::test]
async fn the_last_two_survivors_win_as_lovers() {
    // Cupid links LoverX/LoverY, then dies to the wolf; the village
    // lynches the wolf the next day. LoverX and LoverY are the sole
    // survivors, mutually in love - a Lovers win, which
    // evaluate_winner_with_kills alone could never produce (it has no
    // idea lover pairings exist at all).
    let wolf = PlayerId(1);
    let cupid = PlayerId(2);
    let lover_x = PlayerId(3);
    let lover_y = PlayerId(4);

    let players = roster(&[
        (wolf, Role::Wolf),
        (cupid, Role::Cupid),
        (lover_x, Role::Villager),
        (lover_y, Role::Villager),
    ]);

    // Every voter gets the day's target. The wolf's own vote for itself is
    // refused (nobody can vote for themselves), so the two lovers' votes
    // carry it.
    let mut presenter = Script::new()
        .picks(Prompt::CupidLink, vec![lover_x, lover_y])
        .pick(When::asked(Prompt::WolfEat).in_round(1), cupid)
        .pick(When::asked(Prompt::LynchVote).in_round(1), wolf);
    let mut rng = SeededRngSource::new(0);
    let outcome = run_game(&players, &mut presenter, &mut rng, 10).await;

    assert_eq!(outcome.winner, WinOutcome::Team(Team::Lovers));
    assert_eq!(outcome.rounds_played, 1);
}

#[tokio::test]
async fn shooting_the_wise_elder_still_kills_them_but_demotes_the_gunner() {
    // The Wise Elder does NOT survive a Gunner's shot (confirmed directly
    // against Werewolf.cs - KillPlayer runs unconditionally right after
    // the switch case), but the Gunner is punished for it: demoted to
    // Villager. Both halves, end to end through run_game.
    //
    // Needs a Wolf in the roster - with none at all, "no wolves alive"
    // already resolves a Village win right after night 1, before the day
    // phase (and the Gunner's shot) ever gets a chance to run.
    let wolf = PlayerId(4);
    let gunner = PlayerId(1);
    let wise_elder = PlayerId(2);
    let bystander = PlayerId(3);

    let players = roster(&[
        (wolf, Role::Wolf),
        (gunner, Role::Gunner),
        (wise_elder, Role::WiseElder),
        (bystander, Role::Villager),
    ]);

    let mut presenter = Script::new().pick(When::asked(Prompt::GunnerShoot { bullets: 2 }).by(gunner), wise_elder);
    let mut rng = SeededRngSource::new(0);
    let outcome = run_game(&players, &mut presenter, &mut rng, 1).await;

    assert_eq!(
        outcome.deaths,
        vec![(wise_elder, KillMethod::Shoot)],
        "the Wise Elder should still die from the shot"
    );
    assert!(
        presenter.transformed(gunner, Role::Gunner, Role::Villager),
        "the Gunner should be demoted to Villager for shooting the Wise Elder, got: {:?}",
        presenter.narrated
    );
}

#[tokio::test]
async fn silver_spread_blocks_the_wolf_kill_the_following_night_only() {
    // Blacksmith spreads silver on day 1 (a village-wide yes/no, not a
    // target pick - Werewolf.cs:5083-5092); the wolf's night-2 kill
    // attempt should fail as a result, having succeeded normally on
    // night 1 before the silver was ever spread.
    let wolf = PlayerId(1);
    let blacksmith = PlayerId(2);
    let victim1 = PlayerId(3);
    let victim2 = PlayerId(4);

    let players = roster(&[
        (wolf, Role::Wolf),
        (blacksmith, Role::Blacksmith),
        (victim1, Role::Villager),
        (victim2, Role::Villager),
    ]);

    // Eats victim1 night 1 (before any silver is spread), then tries for
    // victim2 every night after.
    let mut presenter = Script::new()
        .pick(When::asked(Prompt::WolfEat).by(wolf).in_round(1), victim1)
        .pick(When::asked(Prompt::WolfEat).by(wolf), victim2)
        .yes(When::asked(Prompt::BlacksmithSilver).by(blacksmith));
    let mut rng = SeededRngSource::new(0);
    let outcome = run_game(&players, &mut presenter, &mut rng, 2).await;

    assert_eq!(
        outcome.deaths,
        vec![(victim1, KillMethod::Eat)],
        "victim1 should die night 1 (before the silver is spread), and victim2 should survive night 2"
    );
}

#[tokio::test]
async fn sandman_sleep_skips_the_next_night() {
    // The Sandman puts everyone to sleep by day; the following night
    // nobody is asked anything and nobody dies (Werewolf.cs:3011-3020).
    let wolf = PlayerId(1);
    let sandman = PlayerId(2);
    let victim = PlayerId(3);
    let bystander = PlayerId(4);

    let players = roster(&[
        (wolf, Role::Wolf),
        (sandman, Role::Sandman),
        (victim, Role::Villager),
        (bystander, Role::Villager),
        (PlayerId(5), Role::Villager),
    ]);

    let mut presenter = Script::new()
        .pick(When::asked(Prompt::WolfEat).by(wolf), victim)
        .yes(When::asked(Prompt::SandmanSleep).by(sandman));
    let mut rng = SeededRngSource::new(0);
    let outcome = run_game(&players, &mut presenter, &mut rng, 2).await;

    assert_eq!(
        outcome.deaths,
        vec![(victim, KillMethod::Eat)],
        "only night one's kill happens"
    );
    assert_eq!(
        presenter.times_asked(wolf, Prompt::WolfEat),
        1,
        "the wolf has no menu on the night the Sandman skipped"
    );
    for announcement in [Announcement::SandmanSleepAll { sandman }, Announcement::SandmanNight] {
        assert!(presenter.narrated.contains(&NarrationEvent::Announce(announcement)));
    }
}

#[tokio::test]
async fn a_thief_steals_the_targets_role_and_the_target_becomes_a_villager() {
    let wolf = PlayerId(1);
    let thief = PlayerId(2);
    let seer = PlayerId(3);
    let bystander = PlayerId(4);

    let players = roster(&[
        (wolf, Role::Wolf),
        (thief, Role::Thief),
        (seer, Role::Seer),
        (bystander, Role::Villager),
    ]);

    let mut presenter = Script::new().pick(When::asked(Prompt::ThiefSteal).by(thief), seer);
    let mut rng = SeededRngSource::new(0);
    run_game(&players, &mut presenter, &mut rng, 1).await;

    assert!(
        presenter.transformed(thief, Role::Thief, Role::Seer),
        "the Thief should become the Seer, got: {:?}",
        presenter.narrated
    );
    assert!(
        presenter.transformed(seer, Role::Seer, Role::Villager),
        "the stolen-from Seer should become a Villager, got: {:?}",
        presenter.narrated
    );
}

#[tokio::test]
async fn a_harlot_visiting_the_wolves_target_dies_alongside_them() {
    let wolf = PlayerId(1);
    let harlot = PlayerId(2);
    let victim = PlayerId(3);
    let bystander = PlayerId(4);

    let players = roster(&[
        (wolf, Role::Wolf),
        (harlot, Role::Harlot),
        (victim, Role::Villager),
        (bystander, Role::Villager),
    ]);

    let mut presenter = Script::new()
        .pick(When::asked(Prompt::WolfEat).by(wolf), victim)
        .pick(When::asked(Prompt::HarlotVisit).by(harlot), victim);
    let mut rng = SeededRngSource::new(0);
    let outcome = run_game(&players, &mut presenter, &mut rng, 1).await;

    let mut deaths = outcome.deaths.clone();
    deaths.sort_by_key(|(id, _)| id.0);
    assert_eq!(
        deaths,
        vec![(harlot, KillMethod::VisitVictim), (victim, KillMethod::Eat)],
        "visiting the wolves' actual target should kill the Harlot too"
    );
}

#[tokio::test]
async fn a_dead_wolf_cub_grants_the_wolves_a_bonus_kill() {
    let wolf = PlayerId(1);
    let wolf_cub = PlayerId(2);
    let bonus_victim = PlayerId(3);
    let bystander = PlayerId(4);
    let another_bystander = PlayerId(5);

    let players = roster(&[
        (wolf, Role::Wolf),
        (wolf_cub, Role::WolfCub),
        (bonus_victim, Role::Villager),
        (bystander, Role::Villager),
        (another_bystander, Role::Villager),
    ]);

    // Two rounds: the Wolf Cub is lynched day 1, and the bonus kill only
    // lands the *following* night (Werewolf.cs:3030 sends that night's
    // menus before Werewolf.cs:3235 resets WolfCubKilled - confirmed by
    // reading NightCycle directly - so a Wolf Cub who dies during the day
    // can't have already chained the bonus menu into that same day's
    // now-past night).
    let mut presenter = Script::new()
        .pick(When::asked(Prompt::LynchVote).in_round(1), wolf_cub)
        .pick(When::asked(Prompt::WolfCubBonusKill).by(wolf), bonus_victim);
    let mut rng = SeededRngSource::new(0);
    let outcome = run_game(&players, &mut presenter, &mut rng, 2).await;

    let mut deaths = outcome.deaths.clone();
    deaths.sort_by_key(|(id, _)| id.0);
    assert_eq!(
        deaths,
        vec![(wolf_cub, KillMethod::Lynch), (bonus_victim, KillMethod::Eat)],
        "lynching the Wolf Cub should grant the wolves a bonus kill the following night"
    );
    assert_eq!(outcome.rounds_played, 2);
}

#[tokio::test]
async fn a_game_that_never_resolves_hits_the_round_cap_instead_of_hanging() {
    // Nobody ever answers anything -> no deaths, ever -> the win
    // condition never resolves. run_game must still terminate.
    //
    // 1 wolf, 3 villagers: wolves don't outnumber villagers yet, so unlike
    // a 1-wolf-1-villager start (already a resolved "wolves >= others"
    // state before any night even runs), this composition genuinely has
    // no winner until someone dies - and with nobody answering, nobody
    // ever does.
    let players = roster(&[
        (PlayerId(1), Role::Wolf),
        (PlayerId(2), Role::Villager),
        (PlayerId(3), Role::Villager),
        (PlayerId(4), Role::Villager),
    ]);

    let mut presenter = Script::new();
    let mut rng = SeededRngSource::new(0);
    let outcome = run_game(&players, &mut presenter, &mut rng, 3).await;

    assert_eq!(outcome.rounds_played, 3);
    assert!(
        matches!(outcome.winner, WinOutcome::Unimplemented(_)),
        "expected the round-cap outcome, got {:?}",
        outcome.winner
    );
}

/// `run_game` reacts to `Presenter::drain_fled_players`: one player flees
/// the first time it is polled.
#[tokio::test]
async fn fleeing_mid_game_can_immediately_hand_the_wolves_a_win() {
    let wolf = PlayerId(1);
    let villager1 = PlayerId(2);
    let villager2 = PlayerId(3);

    let players = roster(&[
        (wolf, Role::Wolf),
        (villager1, Role::Villager),
        (villager2, Role::Villager),
    ]);

    // Fleeing villager1 leaves 1 wolf vs 1 villager - wolves >= others,
    // an immediate win with no night or day ever needing to resolve.
    let mut presenter = Script::new().flee(villager1);
    let mut rng = SeededRngSource::new(0);
    let outcome = run_game(&players, &mut presenter, &mut rng, 5).await;

    assert_eq!(outcome.winner, WinOutcome::Team(Team::Wolf));
    assert_eq!(outcome.deaths, vec![(villager1, KillMethod::Flee)]);
    assert_eq!(outcome.rounds_played, 1);
}

/// `run_game` reacts to `Presenter::game_was_killed`, here true the first
/// time it is polled.
#[tokio::test]
async fn an_admin_killing_the_game_ends_it_with_no_winner() {
    let wolf = PlayerId(1);
    let villager = PlayerId(2);

    let players = roster(&[(wolf, Role::Wolf), (villager, Role::Villager)]);

    let mut presenter = Script::new().kill_game();
    let mut rng = SeededRngSource::new(0);
    let outcome = run_game(&players, &mut presenter, &mut rng, 5).await;

    assert_eq!(outcome.winner, WinOutcome::Killed);
    assert_eq!(outcome.deaths, vec![]);
    assert_eq!(outcome.rounds_played, 1);
}
