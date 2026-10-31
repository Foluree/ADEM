use super::*;

#[test]
fn height_spring_moves_toward_target_without_overshooting() {
    let target = 40.0;
    let (mut position, mut velocity) = (0.0, 0.0);

    for _ in 0..30 {
        (position, velocity) =
            critically_damped_step(position, velocity, target, 1.0 / 60.0, 20.0);
        assert!((0.0..=target).contains(&position));
        assert!(velocity >= 0.0);
    }

    assert!((position - target).abs() < 0.1);
    assert!(velocity.abs() < 1.0);
}

#[test]
fn height_spring_handles_rapid_target_reversals_without_overshooting() {
    let (mut position, mut velocity) = (0.0, 0.0);
    for target in [40.0, 0.0, 40.0, 0.0, 40.0, 0.0] {
        for _ in 0..3 {
            let previous = position;
            (position, velocity) =
                critically_damped_step(position, velocity, target, 1.0 / 60.0, 20.0);
            assert!((previous.min(target)..=previous.max(target)).contains(&position));
        }
    }
    for _ in 0..60 {
        (position, velocity) =
            critically_damped_step(position, velocity, 0.0, 1.0 / 60.0, 20.0);
    }

    assert!((0.0..=40.0).contains(&position));
    assert!(position < 0.1);
    assert!(velocity.abs() < 1.0);
}
