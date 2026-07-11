//! Browser Use 3D braille orbit-mark logo, ported from browser-use/terminal
//! `crates/browser-use-tui/src/welcome.rs`. Rendered at the canonical resting
//! orientation (no global rotation) as static lines.

const RING_SAMPLES: usize = 120;
const Y_SQUASH_BASE: f32 = 0.55; // monospace cell aspect for 2x4 braille subsampling
const TILT: f32 = std::f32::consts::PI / 3.0;
const ROLL: f32 = std::f32::consts::PI / 4.0;

pub(crate) const LOGO_W: usize = 22;
pub(crate) const LOGO_H: usize = 9;
const LOGO_R: f32 = 14.0;
const LOGO_STROKE: f32 = 1.15;

type M3 = [[f32; 3]; 3];
type V3 = [f32; 3];

fn rot_y(a: f32) -> M3 {
    let (c, s) = (a.cos(), a.sin());
    [[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]]
}

fn rot_z(a: f32) -> M3 {
    let (c, s) = (a.cos(), a.sin());
    [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]]
}

fn mul(a: &M3, b: &M3) -> M3 {
    let mut r = [[0.0_f32; 3]; 3];
    for (i, row) in r.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            for k in 0..3 {
                *cell += a[i][k] * b[k][j];
            }
        }
    }
    r
}

fn apply(m: &M3, v: V3) -> V3 {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

fn ring_points(base: &M3, radius: f32, y_squash: f32) -> Vec<V3> {
    (0..RING_SAMPLES)
        .map(|i| {
            let t = (i as f32 / RING_SAMPLES as f32) * std::f32::consts::PI * 2.0;
            let p = apply(base, [t.cos() * radius, t.sin() * radius, 0.0]);
            [p[0], p[1] * y_squash, p[2]]
        })
        .collect()
}

const BRAILLE_BITS: [[u32; 2]; 4] = [[1, 8], [2, 16], [4, 32], [64, 128]];

/// Render the BU orbit-mark as braille-encoded strings, one per cell row.
pub(crate) fn render_logo_lines() -> Vec<String> {
    let base_a = mul(&rot_z(ROLL), &rot_y(TILT));
    let base_b = mul(&rot_z(-ROLL), &rot_y(TILT));
    let sub_x = 2usize;
    let sub_y = 4usize;
    let sx = LOGO_W * sub_x;
    let sy = LOGO_H * sub_y;
    let cx = sx as f32 / 2.0;
    let cy = sy as f32 / 2.0;
    let y_squash = Y_SQUASH_BASE * (sub_y as f32 / 2.0);

    let pts_a = ring_points(&base_a, LOGO_R, y_squash);
    let pts_b = ring_points(&base_b, LOGO_R, y_squash);

    let stroke2 = LOGO_STROKE * LOGO_STROKE;
    let mut lines = Vec::with_capacity(LOGO_H);

    for cy_idx in 0..LOGO_H {
        let mut row = String::with_capacity(LOGO_W * 3);
        for cx_idx in 0..LOGO_W {
            let mut bits: u32 = 0;
            for (dy, bit_row) in BRAILLE_BITS.iter().enumerate() {
                for (dx, bit) in bit_row.iter().enumerate() {
                    let px = (cx_idx * sub_x + dx) as f32 - cx + 0.5;
                    let py = (cy_idx * sub_y + dy) as f32 - cy + 0.5;
                    let mut min2 = f32::INFINITY;
                    for p in pts_a.iter().chain(pts_b.iter()) {
                        let ddx = p[0] - px;
                        let ddy = p[1] - py;
                        let d = ddx * ddx + ddy * ddy;
                        if d < min2 {
                            min2 = d;
                        }
                    }
                    if min2 < stroke2 {
                        bits |= bit;
                    }
                }
            }
            row.push(char::from_u32(0x2800 + bits).unwrap_or(' '));
        }
        lines.push(row);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logo_has_expected_dimensions_and_ink() {
        let lines = render_logo_lines();
        assert_eq!(lines.len(), LOGO_H);
        for line in &lines {
            assert_eq!(line.chars().count(), LOGO_W);
        }
        let inked = lines
            .iter()
            .flat_map(|l| l.chars())
            .filter(|&c| c != '\u{2800}' && c != ' ')
            .count();
        assert!(inked > 20, "logo should draw the orbit rings, got {inked} inked cells");
    }
}
