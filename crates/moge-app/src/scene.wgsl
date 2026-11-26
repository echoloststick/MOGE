struct Globals {
    vp: mat4x4<f32>,
    sun: vec4<f32>,      // xyz = direction toward the sun, w = Lighting.Brightness
    ambient: vec4<f32>,  // rgb = Ambient, w = fog start
    outdoor: vec4<f32>,  // rgb = OutdoorAmbient, w = fog end
    fog: vec4<f32>,      // rgb = FogColor, w = 1 when the surface is sRGB
    cam_pos: vec4<f32>,
    cam_fwd: vec4<f32>,
    cam_right: vec4<f32>, // right * tan(fov/2) * aspect
    cam_up: vec4<f32>,    // up * tan(fov/2)
    sky: vec4<f32>,       // x = star density, y = sun size, z = moon size, w = celestial bodies (0/1)
};
@group(0) @binding(0) var<uniform> g: Globals;

fn to_out(c: vec3<f32>) -> vec3<f32> {
    // Colors are authored inp display (sRGB) space; sRGB surfaces expect linear values.
    if (g.fog.w > 0.5) { return pow(max(c, vec3<f32>(0.0)), vec3<f32>(2.2)); }
    return c;
}

// ---------------------------------------------------------------- sky
struct SkyOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_sky(@builtin(vertex_index) i: u32) -> SkyOut {
    var p = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    var o: SkyOut;
    o.pos = vec4<f32>(p[i], 1.0, 1.0);
    o.ndc = p[i];
    return o;
}

fn hash31(p: vec3<f32>) -> f32 {
    return fract(sin(dot(p, vec3<f32>(12.9898, 78.233, 37.719))) * 43758.5453);
}

@fragment
fn fs_sky(inp: SkyOut) -> @location(0) vec4<f32> {
    let d = normalize(g.cam_fwd.xyz + g.cam_right.xyz * inp.ndc.x + g.cam_up.xyz * inp.ndc.y);
    let sd = normalize(g.sun.xyz);
    let day = smoothstep(-0.15, 0.2, sd.y);
    let h = clamp(d.y, 0.0, 1.0);

    let day_col = mix(vec3<f32>(0.70, 0.83, 0.96), vec3<f32>(0.30, 0.50, 0.85), pow(h, 0.5));
    let night_col = mix(vec3<f32>(0.035, 0.05, 0.10), vec3<f32>(0.004, 0.008, 0.03), pow(h, 0.5));
    var col = mix(night_col, day_col, day);

    // Sunrise / sunset glow around the sun's azimuth.
    let tw = clamp(1.0 - abs(sd.y) / 0.3, 0.0, 1.0);
    let az_d = normalize(vec3<f32>(d.x, 0.0, d.z) + vec3<f32>(1e-5, 0.0, 0.0));
    let az_s = normalize(vec3<f32>(sd.x, 0.0, sd.z) + vec3<f32>(1e-5, 0.0, 0.0));
    let glow = pow(max(dot(az_d, az_s), 0.0), 3.0) * pow(1.0 - h, 3.0) * tw;
    col = col + vec3<f32>(1.0, 0.55, 0.25) * glow * 0.75;

    // Below the horizon fade toward a darker haze.
    col = mix(col, col * 0.6, clamp(-d.y * 4.0, 0.0, 1.0));

    if (g.sky.w > 0.5) {
        // Stars
        if (d.y > -0.05) {
            let p = d * 90.0;
            let cell = floor(p);
            let r = hash31(cell);
            if (r > 1.0 - g.sky.x) {
                let c = fract(p) - 0.5 - (vec3<f32>(hash31(cell + 1.7), hash31(cell + 5.3), hash31(cell + 9.1)) - 0.5) * 0.6;
                let a = 1.0 - smoothstep(0.0, 0.28, length(c));
                col = col + vec3<f32>(1.0) * a * (1.0 - day) * (0.5 + 0.5 * hash31(cell + 3.3));
            }
        }
        // Sun
        let cs = dot(d, sd);
        let sr = g.sky.y * 0.0025;
        let disk = smoothstep(cos(sr * 1.2), cos(sr * 0.9), cs);
        let halo = exp(-(1.0 - cs) * 90.0) * 0.55;
        let sun_vis = smoothstep(-0.2, 0.05, sd.y);
        col = col + (vec3<f32>(1.0, 0.96, 0.82) * disk + vec3<f32>(1.0, 0.85, 0.55) * halo) * sun_vis;
        // Moon (opposite the sun)
        let cm = dot(d, -sd);
        let mr = g.sky.z * 0.0025;
        let mdisk = smoothstep(cos(mr * 1.2), cos(mr * 0.9), cm);
        let moon_vis = 1.0 - smoothstep(-0.1, 0.1, sd.y);
        col = mix(col, vec3<f32>(0.90, 0.92, 1.0), mdisk * moon_vis);
    }
    return vec4<f32>(to_out(min(col, vec3<f32>(1.0))), 1.0);
}

// ---------------------------------------------------------------- parts
struct PartIn {
    @location(0) pos: vec3<f32>,
    @location(1) nrm: vec3<f32>,
    @location(2) m0: vec4<f32>,
    @location(3) m1: vec4<f32>,
    @location(4) m2: vec4<f32>,
    @location(5) m3: vec4<f32>,
    @location(6) color: vec4<f32>,
    @location(7) params: vec4<f32>,
};

struct PartOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) nrm: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) params: vec4<f32>,
};

@vertex
fn vs_part(inp: PartIn) -> PartOut {
    let model = mat4x4<f32>(inp.m0, inp.m1, inp.m2, inp.m3);
    let w = model * vec4<f32>(inp.pos, 1.0);
    // Normal matrix of R*S: sum(col_i * n_i / |col_i|^2).
    let s0 = max(dot(inp.m0.xyz, inp.m0.xyz), 1e-8);
    let s1 = max(dot(inp.m1.xyz, inp.m1.xyz), 1e-8);
    let s2 = max(dot(inp.m2.xyz, inp.m2.xyz), 1e-8);
    let n = inp.m0.xyz * (inp.nrm.x / s0) + inp.m1.xyz * (inp.nrm.y / s1) + inp.m2.xyz * (inp.nrm.z / s2);
    var o: PartOut;
    o.pos = g.vp * w;
    o.world = w.xyz;
    o.nrm = normalize(n);
    o.color = inp.color;
    o.params = inp.params;
    return o;
}

@fragment
fn fs_part(inp: PartOut) -> @location(0) vec4<f32> {
    var n = normalize(inp.nrm);
    let v = normalize(g.cam_pos.xyz - inp.world);
    if (dot(n, v) < 0.0) { n = -n; } // two-sided: light the face that is visible
    let sd = normalize(g.sun.xyz);
    let day = smoothstep(-0.12, 0.15, sd.y);
    let diff = max(dot(n, sd), 0.0);
    let hemi = (g.ambient.rgb + g.outdoor.rgb) * 0.5 * (0.7 + 0.3 * n.y);
    var light = vec3<f32>(0.30) * (0.3 + 0.7 * day) + hemi * (0.35 + 0.65 * day) + vec3<f32>(1.0, 0.98, 0.94) * diff * min(g.sun.w, 6.0) * 0.22 * day;
    light = min(light, vec3<f32>(1.35));
    var col = inp.color.rgb * light;
    if (inp.params.x > 0.5) { col = inp.color.rgb * 1.15; } // Neon: emissive
    let dist = length(g.cam_pos.xyz - inp.world);
    let span = max(g.outdoor.w - g.ambient.w, 1.0);
    let f = clamp((dist - g.ambient.w) / span, 0.0, 1.0) * select(0.0, 1.0, g.outdoor.w < 90000.0);
    col = mix(col, g.fog.rgb, f);
    return vec4<f32>(to_out(min(col, vec3<f32>(1.0))), inp.color.a);
}
