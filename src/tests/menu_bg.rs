use super::*;

/// Исходник произвольного размера накрывается без полос: копия всегда
/// ровно размера окна.
#[test]
fn cover_image_matches_window_size() {
    let image = Image::new(
        Extent3d {
            width: 100,
            height: 40,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        vec![128; 100 * 40 * 4],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    for window in [
        Vec2::new(1280.0, 720.0),
        Vec2::new(640.0, 960.0),
        Vec2::new(300.0, 300.0),
    ] {
        let cover = build_cover_image(&image, window).expect("копия собирается");
        assert_eq!(cover.width(), window.x as u32);
        assert_eq!(cover.height(), window.y as u32);
    }
}

/// Неизвестный формат пикселей не паникует — фон просто не показывается.
#[test]
fn unexpected_format_is_rejected() {
    let image = Image::new(
        Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        vec![0; 4 * 4 * 8],
        TextureFormat::Rgba16Float,
        RenderAssetUsages::default(),
    );
    assert!(build_cover_image(&image, Vec2::new(64.0, 64.0)).is_none());
}
