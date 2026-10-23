use zoo_core::ZooGame;

fn require_ok(action: String) {
    assert!(
        action.contains("\"ok\":true"),
        "expected successful setup action, got {action}"
    );
}

fn configured_game() -> ZooGame {
    let mut game = ZooGame::new();
    require_ok(game.place_habitat_rect(3, 8, 6, 10));
    require_ok(game.buy_animal_feed());
    require_ok(game.hire_keeper());
    require_ok(game.create_work_zone(0, 0, 19, 13));
    require_ok(game.assign_keeper_zone(1, 1));
    require_ok(game.schedule_keeper(1));
    for _ in 0..4 {
        require_ok(game.adopt(1, "capybara".to_owned()));
    }
    require_ok(game.add_shelter(1));
    game
}

fn main() {
    let mut checksum = 0_u64;

    for run in 0..12_u64 {
        let mut game = configured_game();
        for _ in 0..40 {
            game.tick(240);
        }

        let snapshot = game.snapshot_json();
        checksum = snapshot
            .as_bytes()
            .iter()
            .fold(checksum.wrapping_add(run), |accumulator, byte| {
                accumulator.wrapping_mul(131).wrapping_add(u64::from(*byte))
            });
    }

    println!("zoo-simulation-checksum={checksum}");
}
