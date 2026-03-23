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
    let fine_noise = hash21(floor(uv * vec2<f32>(300.0, 680.0)));
    let coarse_noise = hash21(floor(uv * vec2<f32>(36.0, 120.0)) + vec2<f32>(7.0, 13.0));
    let scan = 0.5 + 0.5 * sin(uv.y * 1500.0);
    let banding = 0.5 + 0.5 * sin(uv.y * 96.0 + uv.x * 8.0);
    let edge = (1.0 - smoothstep(0.0, 0.06, uv.x)) * 0.6;

    let spark = max(fine_noise - 0.93, 0.0) / 0.07;
    let grain_term = (fine_noise - 0.5) * material.settings.x;
    let scan_term = (scan - 0.5) * material.settings.y;
    let drift_term = (coarse_noise - 0.5) * material.settings.z;
    let edge_term = edge * material.settings.w;
    let spark_term = spark * (material.settings.x * 0.75);

    let intensity = clamp(0.004 + grain_term + scan_term + drift_term + edge_term + spark_term, 0.0, 0.045);
    let color = material.tint.rgb * (0.28 + 0.16 * banding + 0.12 * coarse_noise);
    return vec4<f32>(color, intensity);
}
