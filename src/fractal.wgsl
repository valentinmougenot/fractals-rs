// Fractal rendering: a fragment shader evaluates each pixel independently on the GPU.

struct Uniforms {
    center: vec2<f32>,    // view center in the complex plane
    resolution: vec2<f32>, // render area size in pixels
    julia_c: vec2<f32>,   // parameter c (Julia) or (cx, p) (Phoenix)
    scale: f32,           // complex units per pixel
    power: f32,           // exponent for generalized Mandelbrot/Julia
    max_iter: u32,
    kind: u32,
    palette: u32,
    ssaa: u32,            // side of the supersampling grid
    color_scale: f32,
    color_offset: f32,
    linear_out: u32,      // 1 if the render target is sRGB (conversion needed)
    orbit_len: u32,       // > 0: deep zoom (perturbation) with a reference orbit
};

@group(0) @binding(0) var<uniform> u: Uniforms;
// Reference orbit Z_m (computed in f64 on the CPU), used by perturbation.
@group(0) @binding(1) var<storage, read> orbit: array<vec2<f32>>;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>, // origin at top left, 0..1
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    // Fullscreen triangle.
    var p = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    var out: VsOut;
    out.pos = vec4(p[i], 0.0, 1.0);
    out.uv = vec2(p[i].x * 0.5 + 0.5, 0.5 - p[i].y * 0.5);
    return out;
}

const TAU: f32 = 6.28318530718;
const BAILOUT: f32 = 256.0;

// Cosine palettes: a + b * cos(TAU * (c * t + d)). 4 vec3 per palette.
const PALETTES = array<vec3<f32>, 32>(
    // 0 Rainbow
    vec3(0.5, 0.5, 0.5), vec3(0.5, 0.5, 0.5), vec3(1.0, 1.0, 1.0), vec3(0.0, 0.33, 0.67),
    // 1 Ember
    vec3(0.5, 0.5, 0.5), vec3(0.5, 0.5, 0.5), vec3(1.0, 0.7, 0.4), vec3(0.0, 0.15, 0.20),
    // 2 Ocean
    vec3(0.5, 0.5, 0.5), vec3(0.5, 0.5, 0.5), vec3(1.0, 1.0, 0.5), vec3(0.8, 0.9, 0.3),
    // 3 Dusk
    vec3(0.5, 0.5, 0.5), vec3(0.5, 0.5, 0.5), vec3(2.0, 1.0, 0.0), vec3(0.5, 0.2, 0.25),
    // 4 Gold
    vec3(0.8, 0.5, 0.4), vec3(0.2, 0.4, 0.2), vec3(2.0, 1.0, 1.0), vec3(0.0, 0.25, 0.25),
    // 5 Ice
    vec3(0.55, 0.65, 0.75), vec3(0.45, 0.35, 0.25), vec3(1.0, 1.0, 1.0), vec3(0.55, 0.62, 0.70),
    // 6 Forest
    vec3(0.4, 0.5, 0.35), vec3(0.4, 0.4, 0.3), vec3(1.0, 1.0, 0.8), vec3(0.3, 0.1, 0.5),
    // 7 Monochrome
    vec3(0.5, 0.5, 0.5), vec3(0.5, 0.5, 0.5), vec3(1.0, 1.0, 1.0), vec3(0.0, 0.0, 0.0),
);

fn palette(t: f32, idx: u32) -> vec3<f32> {
    let a = PALETTES[idx * 4u];
    let b = PALETTES[idx * 4u + 1u];
    let c = PALETTES[idx * 4u + 2u];
    let d = PALETTES[idx * 4u + 3u];
    return clamp(a + b * cos(TAU * (c * t + d)), vec3(0.0), vec3(1.0));
}

fn cmul(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    return vec2(a.x * b.x - a.y * b.y, a.x * b.y + a.y * b.x);
}

fn cdiv(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    let d = dot(b, b);
    return vec2(a.x * b.x + a.y * b.y, a.y * b.x - a.x * b.y) / d;
}

// z^p for a real exponent (polar form); fast path for p = 2.
fn cpow(z: vec2<f32>, p: f32) -> vec2<f32> {
    if (p == 2.0) {
        return vec2(z.x * z.x - z.y * z.y, 2.0 * z.x * z.y);
    }
    let r2 = dot(z, z);
    if (r2 == 0.0) {
        return vec2(0.0);
    }
    let r = pow(r2, 0.5 * p);
    let a = atan2(z.y, z.x) * p;
    return r * vec2(cos(a), sin(a));
}

// Smoothed escape-time iteration. Returns -1.0 for points inside the set.
fn escape_time(z0: vec2<f32>, c: vec2<f32>, kind: u32) -> f32 {
    var z = z0;
    var prev = vec2(0.0); // z_{n-1}, used by Phoenix
    let p = u.power;
    let log_p = log2(max(abs(p), 1.01));

    for (var i = 0u; i < u.max_iter; i++) {
        var zn: vec2<f32>;
        switch kind {
            case 2u: { // Burning Ship
                let a = abs(z);
                zn = cpow(a, p) + c;
            }
            case 3u: { // Tricorn (Mandelbar)
                zn = cpow(vec2(z.x, -z.y), p) + c;
            }
            case 4u: { // Celtic
                let s = cpow(z, p);
                zn = vec2(abs(s.x), s.y) + c;
            }
            case 6u: { // Phoenix : z' = z² + cx + p * z_{n-1}
                zn = cpow(z, p) + vec2(c.x, 0.0) + c.y * prev;
            }
            default: {
                zn = cpow(z, p) + c;
            }
        }
        prev = z;
        z = zn;
        let r2 = dot(z, z);
        if (r2 > BAILOUT * BAILOUT) {
            // Smoothing: mu = n + 1 - log_p(log2|z|)
            let nu = log2(log2(r2) * 0.5) / log_p;
            return f32(i) + 1.0 - nu;
        }
    }
    return -1.0;
}

// Newton on z^3 - 1: color depends on the root reached and the convergence speed.
fn newton(z0: vec2<f32>) -> vec3<f32> {
    var z = z0;
    let roots = array<vec2<f32>, 3>(
        vec2(1.0, 0.0), vec2(-0.5, 0.8660254), vec2(-0.5, -0.8660254)
    );
    for (var i = 0u; i < u.max_iter; i++) {
        let z2 = cmul(z, z);
        let z3 = cmul(z2, z);
        let f = z3 - vec2(1.0, 0.0);
        let df = 3.0 * z2;
        if (dot(df, df) < 1e-20) { break; }
        z = z - cdiv(f, df);
        for (var k = 0u; k < 3u; k++) {
            let d = z - roots[k];
            let d2 = dot(d, d);
            if (d2 < 1e-6) {
                // Smoothing: estimate the remaining iteration fraction.
                let frac = clamp(log2(1e-6 / max(d2, 1e-30)) * 0.25, 0.0, 1.0);
                let t = (f32(i) + frac) * u.color_scale * 0.05 + u.color_offset + f32(k) / 3.0;
                let shade = 1.0 - 0.6 * clamp((f32(i) + frac) / 30.0, 0.0, 1.0);
                return palette(t, u.palette) * shade;
            }
        }
    }
    return vec3(0.0);
}

// Perturbation (Mandelbrot / Julia, p = 2): iterate only the offset d = z - Z_m from the
// reference orbit: d' = 2·Z·d + d² + dc. Rebasing (d = z - Z_0, m = 0 when |z| < |d| or the
// reference orbit ends) avoids glitch artifacts without glitch detection.
//   Mandelbrot: d0 = 0,      dc = pixel offset
//   Julia:      d0 = offset, dc = 0
fn escape_deep(d0: vec2<f32>, dc: vec2<f32>) -> f32 {
    var d = d0;
    var m = 0u;
    let len = u.orbit_len;
    for (var i = 0u; i < u.max_iter; i++) {
        d = 2.0 * cmul(orbit[m], d) + cmul(d, d) + dc;
        m += 1u;
        let z = orbit[m] + d;
        let r2 = dot(z, z);
        if (r2 > BAILOUT * BAILOUT) {
            let nu = log2(log2(r2) * 0.5);
            return f32(i) + 1.0 - nu;
        }
        if (r2 < dot(d, d) || m + 1u >= len) {
            d = z - orbit[0];
            m = 0u;
        }
    }
    return -1.0;
}

fn shade(c_pix: vec2<f32>, off: vec2<f32>) -> vec3<f32> {
    let kind = u.kind;
    if (kind == 5u) {
        return newton(c_pix);
    }

    var mu: f32;
    switch kind {
        case 1u: { // Julia
            if (u.orbit_len > 0u) {
                mu = escape_deep(off, vec2(0.0));
            } else {
                mu = escape_time(c_pix, u.julia_c, 1u);
            }
        }
        case 6u: { // Phoenix : z0 = point, c = (cx, p)
            mu = escape_time(c_pix, u.julia_c, 6u);
        }
        default: { // Mandelbrot and variants
            // Quick cardioid / main bulb test (p = 2 only).
            if (kind == 0u && u.power == 2.0 && u.orbit_len == 0u) {
                let x = c_pix.x - 0.25;
                let y2 = c_pix.y * c_pix.y;
                let q = x * x + y2;
                if (q * (q + x) <= 0.25 * y2) { return vec3(0.0); }
                let xp = c_pix.x + 1.0;
                if (xp * xp + y2 <= 0.0625) { return vec3(0.0); }
            }
            if (u.orbit_len > 0u && kind == 0u) {
                mu = escape_deep(vec2(0.0), off);
            } else {
                mu = escape_time(vec2(0.0), c_pix, kind);
            }
        }
    }
    if (mu < 0.0) {
        return vec3(0.0);
    }
    let t = mu * u.color_scale * 0.02 + u.color_offset;
    return palette(t, u.palette);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let n = max(u.ssaa, 1u);
    var acc = vec3(0.0);
    for (var sy = 0u; sy < n; sy++) {
        for (var sx = 0u; sx < n; sx++) {
            let sub = (vec2(f32(sx), f32(sy)) + 0.5) / f32(n) - 0.5;
            let px = (in.uv - 0.5) * u.resolution + sub;
            let off = vec2(px.x, -px.y) * u.scale;
            acc += shade(u.center + off, off);
        }
    }
    var col = acc / f32(n * n);
    if (u.linear_out == 1u) {
        col = pow(col, vec3(2.2));
    }
    return vec4(col, 1.0);
}
