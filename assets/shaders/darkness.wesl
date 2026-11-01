struct DarknessInput {
    @builtin(position) position: vec4<f32>,
    @location(2) uv: vec2<f32>,
}

struct DarknessMaterial {
    params: vec4<f32>,
}

@group(2) @binding(0) var<uniform> material: DarknessMaterial;
@group(2) @binding(1) var<storage, read> fire_positions: array<vec4<f32>>;

// Первый огонь с x >= x0 (нижняя граница бинарным поиском). Позиции в
// буфере отсортированы по X — см. update_darkness_overlay.
fn first_fire_at_or_after(x0: f32) -> u32 {
    var lo = 0u;
    var hi = arrayLength(&fire_positions);
    while (lo < hi) {
        let mid = (lo + hi) >> 1u;
        if (fire_positions[mid].x < x0) {
            lo = mid + 1u;
        } else {
            hi = mid;
        }
    }
    return lo;
}

@fragment
fn fragment(in: DarknessInput) -> @location(0) vec4<f32> {
    let darkness = material.params.x;
    let radius = material.params.y;
    var light_strength = 0.0;

    // Окно поиска по X: свет с фиксированным радиусом даёт вклад только от огней, 
    // чей центр не дальше radius по X, поэтому после бинарного поиска достаточно 
    // идти вправо до первого огня за окном. Стоимость фрагмента зависит от числа 
    // огней в колонке, а не от всех источников света буфера.
    let count = arrayLength(&fire_positions);
    var i = first_fire_at_or_after(in.uv.x - radius);
    while (i < count) {
        let fire = fire_positions[i];
        if (fire.x > in.uv.x + radius) {
            break;
        }
        if (fire.z > 0.0) {
            let remaining = 1.0 - distance(in.uv, fire.xy) / radius;
            if (remaining > 0.0) {
                light_strength = max(light_strength, remaining * remaining);
            }
        }
        i += 1u;
    }
    return vec4(0.0, 0.0, 0.0, darkness * (1.0 - light_strength));
}
