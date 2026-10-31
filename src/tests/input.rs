use super::*;

#[test]
fn player_cardinal_input_moves_along_isometric_tile_diagonals() {
    assert_eq!(player_grid_dir((0.0, 1.0)), (1.0, 1.0));
    assert_eq!(player_grid_dir((0.0, -1.0)), (-1.0, -1.0));
    assert_eq!(player_grid_dir((-1.0, 0.0)), (-1.0, 1.0));
    assert_eq!(player_grid_dir((1.0, 0.0)), (1.0, -1.0));
}

#[test]
fn player_diagonal_key_combinations_keep_screen_direction() {
    assert_eq!(player_grid_dir((1.0, 1.0)), (2.0, 0.0));
    assert_eq!(player_grid_dir((-1.0, 1.0)), (0.0, 2.0));
    assert_eq!(player_grid_dir((1.0, -1.0)), (0.0, -2.0));
    assert_eq!(player_grid_dir((-1.0, -1.0)), (-2.0, 0.0));
}
