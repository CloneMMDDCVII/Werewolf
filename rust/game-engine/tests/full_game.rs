//! Proves `run_game` actually plays a game to completion across multiple
//! rounds, not just one isolated night or day — the thing every other
//! integration test in this crate has been building toward.

use async_trait::async_trait;
use game_engine::orchestrator::{AlivePlayer, Presenter, Prompt};
use game_engine::roles::PlayerId;
use game_engine::{run_game, WinOutcome};
use shared::{KillMethod, Role, Team};

/// A presenter with a fixed script per round: the wolf always eats the
/// same target, villagers always vote to lynch the same (wrong) target,
/// so the game plays out deterministically across as many rounds as it
/// takes for the wolves to outnumber the village.
struct FixedScriptPresenter {
    wolf_eats: PlayerId,
    village_lynches: PlayerId,
}

#[async_trait(?Send)]
impl Presenter for FixedScriptPresenter {
    async fn ask_targets(
        &mut self,
        _player: PlayerId,
        prompt: Prompt,
        _options: &[PlayerId],
        count: usize,
    ) -> Option<Vec<PlayerId>> {
        if count != 1 {
            return None;
        }
        match prompt {
            Prompt::WolfEat => Some(vec![self.wolf_eats]),
            Prompt::LynchVote => Some(vec![self.village_lynches]),
            _ => None,
        }
    }
}

#[tokio::test]
async fn wolves_win_once_they_outnumber_the_village() {
    let wolf = PlayerId(1);
    let victim1 = PlayerId(2);
    let victim2 = PlayerId(3);
    let bystander = PlayerId(4);

    let players = vec![
        AlivePlayer {
            id: wolf,
            role: Role::Wolf,
        },
        AlivePlayer {
            id: victim1,
            role: Role::Villager,
        },
        AlivePlayer {
            id: victim2,
            role: Role::Villager,
        },
        AlivePlayer {
            id: bystander,
            role: Role::Villager,
        },
    ];

    // The wolf eats victim1 every night; the village (mis-)votes to lynch
    // victim2 every day. After night 1 + day 1: wolf, bystander alive (2
    // players, 1 wolf) - not yet a majority. The scripts keep targeting
    // players who are already dead once they're gone, which resolve_night/
    // resolve_day naturally drop (dead players aren't in the alive roster
    // asked from), so the game converges to wolf vs. bystander and the
    // wolf wins by outnumbering (1 wolf >= 1 other).
    let mut presenter = FixedScriptPresenter {
        wolf_eats: victim1,
        village_lynches: victim2,
    };

    let outcome = run_game(&players, &mut presenter, 10).await;

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

    let players = vec![
        AlivePlayer {
            id: voter,
            role: Role::Villager,
        },
        AlivePlayer {
            id: tanner,
            role: Role::Tanner,
        },
        AlivePlayer {
            id: wolf,
            role: Role::Wolf,
        },
        AlivePlayer {
            id: villager,
            role: Role::Villager,
        },
    ];

    // Nobody scripted to eat (None), so no night death; the day vote lands
    // on the Tanner, who should win immediately regardless of the wolf
    // still being alive.
    struct LynchTannerPresenter {
        tanner: PlayerId,
    }
    #[async_trait(?Send)]
    impl Presenter for LynchTannerPresenter {
        async fn ask_targets(
            &mut self,
            _player: PlayerId,
            prompt: Prompt,
            _options: &[PlayerId],
            count: usize,
        ) -> Option<Vec<PlayerId>> {
            if count != 1 {
                return None;
            }
            match prompt {
                Prompt::LynchVote => Some(vec![self.tanner]),
                _ => None,
            }
        }
    }

    let mut presenter = LynchTannerPresenter { tanner };
    let outcome = run_game(&players, &mut presenter, 10).await;

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

    let players = vec![
        AlivePlayer {
            id: wolf,
            role: Role::Wolf,
        },
        AlivePlayer {
            id: hunter,
            role: Role::Hunter,
        },
        AlivePlayer {
            id: doomed_villager,
            role: Role::Villager,
        },
        AlivePlayer {
            id: survivor,
            role: Role::Villager,
        },
    ];

    struct HunterRevengePresenter {
        hunter: PlayerId,
        shot: PlayerId,
    }
    #[async_trait(?Send)]
    impl Presenter for HunterRevengePresenter {
        async fn ask_targets(
            &mut self,
            player: PlayerId,
            prompt: Prompt,
            _options: &[PlayerId],
            count: usize,
        ) -> Option<Vec<PlayerId>> {
            if count != 1 {
                return None;
            }
            match prompt {
                Prompt::LynchVote => Some(vec![self.hunter]),
                Prompt::HunterFinalShotLynched if player == self.hunter => Some(vec![self.shot]),
                _ => None,
            }
        }
    }

    let mut presenter = HunterRevengePresenter {
        hunter,
        shot: doomed_villager,
    };
    let outcome = run_game(&players, &mut presenter, 10).await;

    // Only wolf and survivor remain (1 wolf >= 1 other), so the wolf wins
    // - and only because the Hunter's shot claimed a *second* death this
    // round, not just the lynch itself.
    assert_eq!(outcome.winner, WinOutcome::Team(Team::Wolf));
    let mut deaths = outcome.deaths.clone();
    deaths.sort_by_key(|(id, _)| id.0);
    assert_eq!(
        deaths,
        vec![
            (hunter, KillMethod::Lynch),
            (doomed_villager, KillMethod::HunterShot),
        ]
    );
}

/// Scripted presenter for the two Cupid/lover tests below: links two
/// players night 1, then follows a fixed wolf-eat/lynch script.
struct CupidScriptPresenter {
    link: (PlayerId, PlayerId),
    wolf_eats: Vec<PlayerId>, // one entry consumed per night, in order
    lynches: Vec<PlayerId>,   // one entry consumed per day, in order
    night: usize,
    day: usize,
}

#[async_trait(?Send)]
impl Presenter for CupidScriptPresenter {
    async fn ask_targets(
        &mut self,
        _player: PlayerId,
        prompt: Prompt,
        _options: &[PlayerId],
        count: usize,
    ) -> Option<Vec<PlayerId>> {
        match prompt {
            Prompt::CupidLink if count == 2 => Some(vec![self.link.0, self.link.1]),
            Prompt::WolfEat if count == 1 => {
                let target = self.wolf_eats.get(self.night).copied();
                self.night += 1;
                target.map(|t| vec![t])
            }
            Prompt::LynchVote if count == 1 => {
                let target = self.lynches.get(self.day).copied();
                self.day += 1;
                target.map(|t| vec![t])
            }
            _ => None,
        }
    }
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

    let players = vec![
        AlivePlayer { id: wolf, role: Role::Wolf },
        AlivePlayer { id: cupid, role: Role::Cupid },
        AlivePlayer { id: lover_x, role: Role::Villager },
        AlivePlayer { id: lover_y, role: Role::Villager },
    ];

    let mut presenter = CupidScriptPresenter {
        link: (lover_x, lover_y),
        wolf_eats: vec![lover_x],
        lynches: vec![],
        night: 0,
        day: 0,
    };
    let outcome = run_game(&players, &mut presenter, 1).await;

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

    let players = vec![
        AlivePlayer { id: wolf, role: Role::Wolf },
        AlivePlayer { id: cupid, role: Role::Cupid },
        AlivePlayer { id: lover_x, role: Role::Villager },
        AlivePlayer { id: lover_y, role: Role::Villager },
    ];

    let mut presenter = CupidScriptPresenter {
        link: (lover_x, lover_y),
        wolf_eats: vec![cupid],
        lynches: vec![wolf],
        night: 0,
        day: 0,
    };
    let outcome = run_game(&players, &mut presenter, 10).await;

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

    let players = vec![
        AlivePlayer { id: wolf, role: Role::Wolf },
        AlivePlayer { id: gunner, role: Role::Gunner },
        AlivePlayer { id: wise_elder, role: Role::WiseElder },
        AlivePlayer { id: bystander, role: Role::Villager },
    ];

    struct ShootWiseElderPresenter {
        gunner: PlayerId,
        wise_elder: PlayerId,
        transforms: Vec<game_engine::orchestrator::NarrationEvent>,
    }
    #[async_trait(?Send)]
    impl Presenter for ShootWiseElderPresenter {
        async fn ask_targets(
            &mut self,
            player: PlayerId,
            prompt: Prompt,
            _options: &[PlayerId],
            count: usize,
        ) -> Option<Vec<PlayerId>> {
            if count != 1 {
                return None;
            }
            match prompt {
                Prompt::GunnerShoot if player == self.gunner => Some(vec![self.wise_elder]),
                _ => None,
            }
        }

        async fn narrate(&mut self, event: game_engine::orchestrator::NarrationEvent) {
            self.transforms.push(event);
        }
    }

    let mut presenter = ShootWiseElderPresenter {
        gunner,
        wise_elder,
        transforms: vec![],
    };
    let outcome = run_game(&players, &mut presenter, 1).await;

    assert_eq!(
        outcome.deaths,
        vec![(wise_elder, KillMethod::Shoot)],
        "the Wise Elder should still die from the shot"
    );
    assert!(
        presenter.transforms.iter().any(|e| matches!(
            e,
            game_engine::orchestrator::NarrationEvent::Transform {
                player,
                from: Role::Gunner,
                to: Role::Villager,
            } if *player == gunner
        )),
        "the Gunner should be demoted to Villager for shooting the Wise Elder, got: {:?}",
        presenter.transforms
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

    let players = vec![
        AlivePlayer { id: wolf, role: Role::Wolf },
        AlivePlayer { id: blacksmith, role: Role::Blacksmith },
        AlivePlayer { id: victim1, role: Role::Villager },
        AlivePlayer { id: victim2, role: Role::Villager },
    ];

    struct SilverPresenter {
        wolf: PlayerId,
        blacksmith: PlayerId,
        victim1: PlayerId,
        victim2: PlayerId,
        night: u32,
    }
    #[async_trait(?Send)]
    impl Presenter for SilverPresenter {
        async fn ask_targets(
            &mut self,
            player: PlayerId,
            prompt: Prompt,
            _options: &[PlayerId],
            count: usize,
        ) -> Option<Vec<PlayerId>> {
            if count != 1 || prompt != Prompt::WolfEat || player != self.wolf {
                return None;
            }
            // Eats victim1 night 1 (before any silver is spread), then
            // tries for victim2 every night after.
            Some(vec![if self.night <= 1 { self.victim1 } else { self.victim2 }])
        }

        async fn ask_toggle(&mut self, player: PlayerId, prompt: Prompt) -> bool {
            prompt == Prompt::BlacksmithSilver && player == self.blacksmith
        }

        fn advance_round(&mut self) {
            self.night += 1;
        }
    }

    let mut presenter = SilverPresenter {
        wolf,
        blacksmith,
        victim1,
        victim2,
        night: 1,
    };
    let outcome = run_game(&players, &mut presenter, 2).await;

    assert_eq!(
        outcome.deaths,
        vec![(victim1, KillMethod::Eat)],
        "victim1 should die night 1 (before the silver is spread), and victim2 should survive night 2"
    );
}

#[tokio::test]
async fn sandman_sleep_cancels_the_whole_nights_deaths() {
    // Sandman puts everyone to sleep the same night the wolf tries to
    // eat someone - the kill should never happen.
    let wolf = PlayerId(1);
    let sandman = PlayerId(2);
    let victim = PlayerId(3);
    let bystander = PlayerId(4);

    let players = vec![
        AlivePlayer { id: wolf, role: Role::Wolf },
        AlivePlayer { id: sandman, role: Role::Sandman },
        AlivePlayer { id: victim, role: Role::Villager },
        AlivePlayer { id: bystander, role: Role::Villager },
    ];

    struct SandmanPresenter {
        wolf: PlayerId,
        sandman: PlayerId,
        victim: PlayerId,
    }
    #[async_trait(?Send)]
    impl Presenter for SandmanPresenter {
        async fn ask_targets(
            &mut self,
            player: PlayerId,
            prompt: Prompt,
            _options: &[PlayerId],
            count: usize,
        ) -> Option<Vec<PlayerId>> {
            if count != 1 || prompt != Prompt::WolfEat || player != self.wolf {
                return None;
            }
            Some(vec![self.victim])
        }

        async fn ask_toggle(&mut self, player: PlayerId, prompt: Prompt) -> bool {
            prompt == Prompt::SandmanSleep && player == self.sandman
        }
    }

    let mut presenter = SandmanPresenter { wolf, sandman, victim };
    let outcome = run_game(&players, &mut presenter, 1).await;

    assert_eq!(
        outcome.deaths,
        vec![],
        "a Sandman sleep should cancel the wolf's kill entirely, end to end"
    );
}

#[tokio::test]
async fn a_thief_steals_the_targets_role_and_the_target_becomes_a_villager() {
    let wolf = PlayerId(1);
    let thief = PlayerId(2);
    let seer = PlayerId(3);
    let bystander = PlayerId(4);

    let players = vec![
        AlivePlayer { id: wolf, role: Role::Wolf },
        AlivePlayer { id: thief, role: Role::Thief },
        AlivePlayer { id: seer, role: Role::Seer },
        AlivePlayer { id: bystander, role: Role::Villager },
    ];

    struct StealPresenter {
        thief: PlayerId,
        seer: PlayerId,
        transforms: Vec<game_engine::orchestrator::NarrationEvent>,
    }
    #[async_trait(?Send)]
    impl Presenter for StealPresenter {
        async fn ask_targets(
            &mut self,
            player: PlayerId,
            prompt: Prompt,
            _options: &[PlayerId],
            count: usize,
        ) -> Option<Vec<PlayerId>> {
            if count != 1 {
                return None;
            }
            match prompt {
                Prompt::ThiefSteal if player == self.thief => Some(vec![self.seer]),
                _ => None,
            }
        }

        async fn narrate(&mut self, event: game_engine::orchestrator::NarrationEvent) {
            self.transforms.push(event);
        }
    }

    let mut presenter = StealPresenter {
        thief,
        seer,
        transforms: vec![],
    };
    run_game(&players, &mut presenter, 1).await;

    assert!(
        presenter.transforms.iter().any(|e| matches!(
            e,
            game_engine::orchestrator::NarrationEvent::Transform { player, from: Role::Thief, to: Role::Seer }
                if *player == thief
        )),
        "the Thief should become the Seer, got: {:?}",
        presenter.transforms
    );
    assert!(
        presenter.transforms.iter().any(|e| matches!(
            e,
            game_engine::orchestrator::NarrationEvent::Transform { player, from: Role::Seer, to: Role::Villager }
                if *player == seer
        )),
        "the stolen-from Seer should become a Villager, got: {:?}",
        presenter.transforms
    );
}

#[tokio::test]
async fn a_harlot_visiting_the_wolves_target_dies_alongside_them() {
    let wolf = PlayerId(1);
    let harlot = PlayerId(2);
    let victim = PlayerId(3);
    let bystander = PlayerId(4);

    let players = vec![
        AlivePlayer { id: wolf, role: Role::Wolf },
        AlivePlayer { id: harlot, role: Role::Harlot },
        AlivePlayer { id: victim, role: Role::Villager },
        AlivePlayer { id: bystander, role: Role::Villager },
    ];

    struct HarlotVisitsVictimPresenter {
        wolf: PlayerId,
        harlot: PlayerId,
        victim: PlayerId,
    }
    #[async_trait(?Send)]
    impl Presenter for HarlotVisitsVictimPresenter {
        async fn ask_targets(
            &mut self,
            player: PlayerId,
            prompt: Prompt,
            _options: &[PlayerId],
            count: usize,
        ) -> Option<Vec<PlayerId>> {
            if count != 1 {
                return None;
            }
            match prompt {
                Prompt::WolfEat if player == self.wolf => Some(vec![self.victim]),
                Prompt::HarlotVisit if player == self.harlot => Some(vec![self.victim]),
                _ => None,
            }
        }
    }

    let mut presenter = HarlotVisitsVictimPresenter { wolf, harlot, victim };
    let outcome = run_game(&players, &mut presenter, 1).await;

    let mut deaths = outcome.deaths.clone();
    deaths.sort_by_key(|(id, _)| id.0);
    assert_eq!(
        deaths,
        vec![(harlot, KillMethod::VisitWolf), (victim, KillMethod::Eat)],
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

    let players = vec![
        AlivePlayer { id: wolf, role: Role::Wolf },
        AlivePlayer { id: wolf_cub, role: Role::WolfCub },
        AlivePlayer { id: bonus_victim, role: Role::Villager },
        AlivePlayer { id: bystander, role: Role::Villager },
        AlivePlayer { id: another_bystander, role: Role::Villager },
    ];

    struct WolfCubBonusPresenter {
        wolf_cub: PlayerId,
        wolf: PlayerId,
        bonus_victim: PlayerId,
    }
    #[async_trait(?Send)]
    impl Presenter for WolfCubBonusPresenter {
        async fn ask_targets(
            &mut self,
            player: PlayerId,
            prompt: Prompt,
            _options: &[PlayerId],
            count: usize,
        ) -> Option<Vec<PlayerId>> {
            if count != 1 {
                return None;
            }
            match prompt {
                Prompt::LynchVote => Some(vec![self.wolf_cub]),
                Prompt::WolfCubBonusKill if player == self.wolf => Some(vec![self.bonus_victim]),
                _ => None,
            }
        }
    }

    let mut presenter = WolfCubBonusPresenter { wolf_cub, wolf, bonus_victim };
    let outcome = run_game(&players, &mut presenter, 1).await;

    let mut deaths = outcome.deaths.clone();
    deaths.sort_by_key(|(id, _)| id.0);
    assert_eq!(
        deaths,
        vec![(wolf_cub, KillMethod::Lynch), (bonus_victim, KillMethod::Eat)],
        "lynching the Wolf Cub should grant the wolves a bonus kill"
    );
}

#[tokio::test]
async fn a_game_that_never_resolves_hits_the_round_cap_instead_of_hanging() {
    // Nobody ever answers anything -> no deaths, ever -> the win
    // condition never resolves. run_game must still terminate.
    struct SilentPresenter;
    #[async_trait(?Send)]
    impl Presenter for SilentPresenter {
        async fn ask_targets(
            &mut self,
            _player: PlayerId,
            _prompt: Prompt,
            _options: &[PlayerId],
            _count: usize,
        ) -> Option<Vec<PlayerId>> {
            None
        }
    }

    // 1 wolf, 3 villagers: wolves don't outnumber villagers yet, so unlike
    // a 1-wolf-1-villager start (already a resolved "wolves >= others"
    // state before any night even runs), this composition genuinely has
    // no winner until someone dies - and with SilentPresenter, nobody ever
    // does.
    let players = vec![
        AlivePlayer {
            id: PlayerId(1),
            role: Role::Wolf,
        },
        AlivePlayer {
            id: PlayerId(2),
            role: Role::Villager,
        },
        AlivePlayer {
            id: PlayerId(3),
            role: Role::Villager,
        },
        AlivePlayer {
            id: PlayerId(4),
            role: Role::Villager,
        },
    ];

    let mut presenter = SilentPresenter;
    let outcome = run_game(&players, &mut presenter, 3).await;

    assert_eq!(outcome.rounds_played, 3);
    assert!(
        matches!(outcome.winner, WinOutcome::Unimplemented(_)),
        "expected the round-cap outcome, got {:?}",
        outcome.winner
    );
}
