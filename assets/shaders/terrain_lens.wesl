import bevy_pbr::render::{
    forward_io::VertexOutput,
    pbr_fragment::pbr_input_from_standard_material,
};

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> @location(0) vec4<f32> {
    let pbr_input = pbr_input_from_standard_material(in, is_front);
    var color = pbr_input.material.base_color.rgb;

    if in.color.r < 0.999 || in.color.g < 0.999 || in.color.b < 0.999 {
        color = (color - vec3<f32>(0.5)) * 0.75 + vec3<f32>(0.5);
    }

    return vec4<f32>(color, 1.0);
}
