#import bevy_sprite::mesh2d_vertex_output::VertexOutput

struct AgentListAnalogMaterial {
    tint: vec4<f32>,
    settings: vec4<f32>,
}

@group(2) @binding(0) var<uniform> material: AgentListAnalogMaterial;

fn hash21(p: vec2<f32>) -> f32 {
    var q = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    q += dot(q, q.yzx + 33.33);
    return fract((q.x + q.y) * q.z);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let fine_noise = hash21(floor(uv * vec2<f32>(240.0, 540.0)));
    let coarse_noise = hash21(floor(uv * vec2<f32>(24.0, 90.0)) + vec2<f32>(7.0, 13.0));
    let scan = 0.5 + 0.5 * sin(uv.y * 1300.0);
    let banding = 0.5 + 0.5 * sin(uv.y * 92.0 + uv.x * 8.0);
    let edge = (1.0 - smoothstep(0.0, 0.09, uv.x)) * 0.8;

    let grain_term = (fine_noise - 0.5) * material.settings.x;
    let scan_term = (scan - 0.5) * material.settings.y;
    let drift_term = (coarse_noise - 0.5) * material.settings.z;
    let edge_term = edge * material.settings.w;

    let intensity = clamp(0.045 + grain_term + scan_term + drift_term + edge_term, 0.0, 0.18);
    let color = material.tint.rgb * (0.65 + 0.45 * banding + 0.25 * coarse_noise);
    return vec4<f32>(color, intensity);
}
