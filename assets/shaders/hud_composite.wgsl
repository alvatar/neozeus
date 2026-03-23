#import bevy_render::view::View

@group(0) @binding(0)
var<uniform> view: View;
@group(2) @binding(0)
var hud_texture: texture_2d<f32>;
@group(2) @binding(1)
var hud_sampler: sampler;

fn coords_to_viewport_uv(position: vec2<f32>, viewport: vec4<f32>) -> vec2<f32> {
    return (position - viewport.xy) / viewport.zw;
}

fn srgb_to_linear_channel(value: f32) -> f32 {
    if value <= 0.04045 {
        return value / 12.92;
    }
    return pow((value + 0.055) / 1.055, 2.4);
}

fn srgb_to_linear(rgb: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        srgb_to_linear_channel(rgb.r),
        srgb_to_linear_channel(rgb.g),
        srgb_to_linear_channel(rgb.b),
    );
}

@fragment
fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = coords_to_viewport_uv(position.xy, view.viewport);
    let sample = textureSample(hud_texture, hud_sampler, uv);
    return vec4<f32>(srgb_to_linear(sample.rgb), sample.a);
}
