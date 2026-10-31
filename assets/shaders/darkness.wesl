struct DarknessInput {
    @builtin(position) position: vec4<f32>,
    @location(2) uv: vec2<f32>,
}

struct DarknessMaterial {
    params: vec4<f32>,
}

@group(2) @binding(0) var<uniform> material: DarknessMaterial;
@group(2) @binding(1) var<storage, read> fire_positions: array<vec4<f32>>;

@fragment
fn fragment(in: DarknessInput) -> @location(0) vec4<f32> {
    let darkness = material.params.x;
    let radius = material.params.y;
    var light_strength = 0.0;

    for (var i = 0u; i < arrayLength(&fire_positions); i += 1u) {
        if (fire_positions[i].z > 0.0) {
            let remaining = 1.0 - distance(in.uv, fire_positions[i].xy) / radius;
            if (remaining > 0.0) {
                light_strength = max(light_strength, remaining * remaining);
            }
        }
    }

    return vec4(0.0, 0.0, 0.0, darkness * (1.0 - light_strength));
}
