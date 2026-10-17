//! Fractal kinds, presets and palettes.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Mandelbrot = 0,
    Julia = 1,
    BurningShip = 2,
    Tricorn = 3,
    Celtic = 4,
    Newton = 5,
    Phoenix = 6,
}

impl Kind {
    pub const ALL: [Kind; 7] = [
        Kind::Mandelbrot,
        Kind::Julia,
        Kind::BurningShip,
        Kind::Tricorn,
        Kind::Celtic,
        Kind::Newton,
        Kind::Phoenix,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Mandelbrot => "Mandelbrot",
            Kind::Julia => "Julia",
            Kind::BurningShip => "Burning Ship",
            Kind::Tricorn => "Tricorn",
            Kind::Celtic => "Celtic",
            Kind::Newton => "Newton (z³ − 1)",
            Kind::Phoenix => "Phoenix",
        }
    }

    /// Initial view: (center, view height in complex units).
    pub fn default_view(self) -> ([f64; 2], f64) {
        match self {
            Kind::Mandelbrot => ([-0.5, 0.0], 3.0),
            Kind::Julia => ([0.0, 0.0], 3.2),
            Kind::BurningShip => ([-0.4, -0.5], 3.2),
            Kind::Tricorn => ([0.0, 0.0], 3.4),
            Kind::Celtic => ([-0.3, 0.0], 3.4),
            Kind::Newton => ([0.0, 0.0], 3.0),
            Kind::Phoenix => ([0.0, 0.0], 3.0),
        }
    }

    pub fn has_power(self) -> bool {
        !matches!(self, Kind::Newton)
    }

    /// Whether the parameter c (or (cx, p) for Phoenix) is used.
    pub fn has_c(self) -> bool {
        matches!(self, Kind::Julia | Kind::Phoenix)
    }
}

pub struct JuliaPreset {
    pub name: &'static str,
    pub c: [f64; 2],
}

pub const JULIA_PRESETS: &[JuliaPreset] = &[
    JuliaPreset { name: "Douady rabbit", c: [-0.123, 0.745] },
    JuliaPreset { name: "San Marco", c: [-0.75, 0.0] },
    JuliaPreset { name: "Dendrite", c: [0.0, 1.0] },
    JuliaPreset { name: "Siegel disk", c: [-0.391, -0.587] },
    JuliaPreset { name: "Galaxy", c: [-0.8, 0.156] },
    JuliaPreset { name: "Spiral", c: [-0.7269, 0.1889] },
    JuliaPreset { name: "Airplane", c: [-1.755, 0.0] },
    JuliaPreset { name: "Fatou dust", c: [0.285, 0.01] },
    JuliaPreset { name: "Snowflake", c: [-0.4, 0.6] },
];

pub const PHOENIX_C: [f64; 2] = [0.5667, -0.5];

pub struct Place {
    pub name: &'static str,
    pub center: [f64; 2],
    pub height: f64,
}

/// Famous places of the Mandelbrot set (the last two use deep zoom).
pub const MANDELBROT_PLACES: &[Place] = &[
    Place { name: "Overview", center: [-0.5, 0.0], height: 3.0 },
    Place { name: "Seahorse Valley", center: [-0.75, 0.1], height: 0.2 },
    Place { name: "Elephant Valley", center: [0.275, 0.0], height: 0.1 },
    Place { name: "Double spiral", center: [-0.7616, -0.0847], height: 0.02 },
    Place { name: "Mini-Mandelbrot", center: [-1.7548777, 0.0], height: 0.03 },
    Place { name: "Seahorses (deep)", center: [-0.743643887037158704752191506114774, 0.131825904205311970493132056385139], height: 4.0e-10 },
    Place { name: "Mini-Mandelbrot (deep)", center: [-1.995419032661308, 0.0], height: 3.0e-7 },
];

/// Cosine palettes (a, b, c, d); keep in sync with `fractal.wgsl`.
pub const PALETTES: &[(&str, [[f32; 3]; 4])] = &[
    ("Rainbow", [[0.5, 0.5, 0.5], [0.5, 0.5, 0.5], [1.0, 1.0, 1.0], [0.0, 0.33, 0.67]]),
    ("Ember", [[0.5, 0.5, 0.5], [0.5, 0.5, 0.5], [1.0, 0.7, 0.4], [0.0, 0.15, 0.20]]),
    ("Ocean", [[0.5, 0.5, 0.5], [0.5, 0.5, 0.5], [1.0, 1.0, 0.5], [0.8, 0.9, 0.3]]),
    ("Dusk", [[0.5, 0.5, 0.5], [0.5, 0.5, 0.5], [2.0, 1.0, 0.0], [0.5, 0.2, 0.25]]),
    ("Gold", [[0.8, 0.5, 0.4], [0.2, 0.4, 0.2], [2.0, 1.0, 1.0], [0.0, 0.25, 0.25]]),
    ("Ice", [[0.55, 0.65, 0.75], [0.45, 0.35, 0.25], [1.0, 1.0, 1.0], [0.55, 0.62, 0.70]]),
    ("Forest", [[0.4, 0.5, 0.35], [0.4, 0.4, 0.3], [1.0, 1.0, 0.8], [0.3, 0.1, 0.5]]),
    ("Monochrome", [[0.5, 0.5, 0.5], [0.5, 0.5, 0.5], [1.0, 1.0, 1.0], [0.0, 0.0, 0.0]]),
];

pub fn palette_color(idx: usize, t: f32) -> [u8; 3] {
    let [a, b, c, d] = PALETTES[idx].1;
    let mut out = [0u8; 3];
    for i in 0..3 {
        let v = a[i] + b[i] * (std::f32::consts::TAU * (c[i] * t + d[i])).cos();
        out[i] = (v.clamp(0.0, 1.0) * 255.0) as u8;
    }
    out
}
