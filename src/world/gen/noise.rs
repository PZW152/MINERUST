//! Ports of NoiseGeneratorImproved / Octaves / Simplex / Perlin.

use crate::java::JavaRandom;
use crate::util::math::floor_double_long;

const GRAD_X: [f64; 16] = [
    1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -1.0, 0.0,
];
const GRAD_Y: [f64; 16] = [
    1.0, 1.0, -1.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0,
];
const GRAD_Z: [f64; 16] = [
    0.0, 0.0, 0.0, 0.0, 1.0, 1.0, -1.0, -1.0, 1.0, 1.0, -1.0, -1.0, 0.0, 1.0, 0.0, -1.0,
];

pub struct NoiseGeneratorImproved {
    p: [i32; 512],
    pub x_coord: f64,
    pub y_coord: f64,
    pub z_coord: f64,
}

#[inline]
fn lerp(t: f64, a: f64, b: f64) -> f64 {
    a + t * (b - a)
}

#[inline]
fn grad2(h: i32, x: f64, z: f64) -> f64 {
    let i = (h & 15) as usize;
    GRAD_X[i] * x + GRAD_Z[i] * z
}

#[inline]
fn grad(h: i32, x: f64, y: f64, z: f64) -> f64 {
    let i = (h & 15) as usize;
    GRAD_X[i] * x + GRAD_Y[i] * y + GRAD_Z[i] * z
}

fn init_perm(rand: &mut JavaRandom) -> (f64, f64, f64, [i32; 512]) {
    let x = rand.next_double() * 256.0;
    let y = rand.next_double() * 256.0;
    let z = rand.next_double() * 256.0;
    let mut p = [0i32; 512];
    for i in 0..256 {
        p[i] = i as i32;
    }
    for i in 0..256usize {
        let j = (rand.next_int_n(256 - i as i32) + i as i32) as usize;
        p.swap(i, j);
        p[i + 256] = p[i];
    }
    (x, y, z, p)
}

impl NoiseGeneratorImproved {
    pub fn new(rand: &mut JavaRandom) -> Self {
        let (x, y, z, p) = init_perm(rand);
        NoiseGeneratorImproved {
            p,
            x_coord: x,
            y_coord: y,
            z_coord: z,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn populate_noise_array(
        &self,
        arr: &mut [f64],
        xo: f64,
        yo: f64,
        zo: f64,
        xs: i32,
        ys: i32,
        zs: i32,
        sx: f64,
        sy: f64,
        sz: f64,
        noise_scale: f64,
    ) {
        let p = &self.p;
        if ys == 1 {
            let mut idx = 0usize;
            let inv = 1.0 / noise_scale;
            for ix in 0..xs {
                let mut x = xo + ix as f64 * sx + self.x_coord;
                let mut xi = x as i32;
                if x < xi as f64 {
                    xi -= 1;
                }
                let xb = (xi & 0xFF) as usize;
                x -= xi as f64;
                let fx = x * x * x * (x * (x * 6.0 - 15.0) + 10.0);
                for iz in 0..zs {
                    let mut z = zo + iz as f64 * sz + self.z_coord;
                    let mut zi = z as i32;
                    if z < zi as f64 {
                        zi -= 1;
                    }
                    let zb = zi & 0xFF;
                    z -= zi as f64;
                    let fz = z * z * z * (z * (z * 6.0 - 15.0) + 10.0);
                    let a = p[xb];
                    let aa = (p[a as usize] + zb) as usize;
                    let b = p[xb + 1];
                    let ba = (p[b as usize] + zb) as usize;
                    let l1 = lerp(fx, grad2(p[aa], x, z), grad(p[ba], x - 1.0, 0.0, z));
                    let l2 = lerp(
                        fx,
                        grad(p[aa + 1], x, 0.0, z - 1.0),
                        grad(p[ba + 1], x - 1.0, 0.0, z - 1.0),
                    );
                    let v = lerp(fz, l1, l2);
                    arr[idx] += v * inv;
                    idx += 1;
                }
            }
        } else {
            let mut idx = 0usize;
            let inv = 1.0 / noise_scale;
            let mut last_y = -1i32;
            let (mut l1, mut l2, mut l3, mut l4) = (0.0, 0.0, 0.0, 0.0);
            for ix in 0..xs {
                let mut x = xo + ix as f64 * sx + self.x_coord;
                let mut xi = x as i32;
                if x < xi as f64 {
                    xi -= 1;
                }
                let xb = (xi & 0xFF) as usize;
                x -= xi as f64;
                let fx = x * x * x * (x * (x * 6.0 - 15.0) + 10.0);
                for iz in 0..zs {
                    let mut z = zo + iz as f64 * sz + self.z_coord;
                    let mut zi = z as i32;
                    if z < zi as f64 {
                        zi -= 1;
                    }
                    let zb = zi & 0xFF;
                    z -= zi as f64;
                    let fz = z * z * z * (z * (z * 6.0 - 15.0) + 10.0);
                    for iy in 0..ys {
                        let mut y = yo + iy as f64 * sy + self.y_coord;
                        let mut yi = y as i32;
                        if y < yi as f64 {
                            yi -= 1;
                        }
                        let yb = yi & 0xFF;
                        y -= yi as f64;
                        let fy = y * y * y * (y * (y * 6.0 - 15.0) + 10.0);
                        if iy == 0 || yb != last_y {
                            last_y = yb;
                            let a = (p[xb] + yb) as usize;
                            let aa = (p[a] + zb) as usize;
                            let ab = (p[a + 1] + zb) as usize;
                            let b = (p[xb + 1] + yb) as usize;
                            let ba = (p[b] + zb) as usize;
                            let bb = (p[b + 1] + zb) as usize;
                            l1 = lerp(fx, grad(p[aa], x, y, z), grad(p[ba], x - 1.0, y, z));
                            l2 = lerp(
                                fx,
                                grad(p[ab], x, y - 1.0, z),
                                grad(p[bb], x - 1.0, y - 1.0, z),
                            );
                            l3 = lerp(
                                fx,
                                grad(p[aa + 1], x, y, z - 1.0),
                                grad(p[ba + 1], x - 1.0, y, z - 1.0),
                            );
                            l4 = lerp(
                                fx,
                                grad(p[ab + 1], x, y - 1.0, z - 1.0),
                                grad(p[bb + 1], x - 1.0, y - 1.0, z - 1.0),
                            );
                        }
                        let m1 = lerp(fy, l1, l2);
                        let m2 = lerp(fy, l3, l4);
                        let v = lerp(fz, m1, m2);
                        arr[idx] += v * inv;
                        idx += 1;
                    }
                }
            }
        }
    }
}

pub struct NoiseGeneratorOctaves {
    gens: Vec<NoiseGeneratorImproved>,
}

impl NoiseGeneratorOctaves {
    pub fn new(rand: &mut JavaRandom, octaves: usize) -> Self {
        NoiseGeneratorOctaves {
            gens: (0..octaves)
                .map(|_| NoiseGeneratorImproved::new(rand))
                .collect(),
        }
    }

    /// 3D form. `arr` is zeroed and must be `xs*ys*zs` long.
    #[allow(clippy::too_many_arguments)]
    pub fn generate(
        &self,
        arr: &mut [f64],
        x: i32,
        y: i32,
        z: i32,
        xs: i32,
        ys: i32,
        zs: i32,
        sx: f64,
        sy: f64,
        sz: f64,
    ) {
        for v in arr.iter_mut() {
            *v = 0.0;
        }
        let mut scale = 1.0f64;
        for g in &self.gens {
            let mut dx = x as f64 * scale * sx;
            let dy = y as f64 * scale * sy;
            let mut dz = z as f64 * scale * sz;
            let mut lx = floor_double_long(dx);
            let mut lz = floor_double_long(dz);
            dx -= lx as f64;
            dz -= lz as f64;
            lx %= 16777216;
            lz %= 16777216;
            dx += lx as f64;
            dz += lz as f64;
            g.populate_noise_array(
                arr,
                dx,
                dy,
                dz,
                xs,
                ys,
                zs,
                sx * scale,
                sy * scale,
                sz * scale,
                scale,
            );
            scale /= 2.0;
        }
    }

    /// 2D overload.
    #[allow(clippy::too_many_arguments)]
    pub fn generate_2d(
        &self,
        arr: &mut [f64],
        x: i32,
        z: i32,
        xs: i32,
        zs: i32,
        sx: f64,
        sz: f64,
        _unused: f64,
    ) {
        self.generate(arr, x, 10, z, xs, 1, zs, sx, 1.0, sz);
    }
}

const GRAD3: [[i32; 3]; 12] = [
    [1, 1, 0],
    [-1, 1, 0],
    [1, -1, 0],
    [-1, -1, 0],
    [1, 0, 1],
    [-1, 0, 1],
    [1, 0, -1],
    [-1, 0, -1],
    [0, 1, 1],
    [0, -1, 1],
    [0, 1, -1],
    [0, -1, -1],
];

pub struct NoiseGeneratorSimplex {
    p: [i32; 512],
    pub xo: f64,
    pub yo: f64,
    pub zo: f64,
}

#[inline]
fn fast_floor(d: f64) -> i32 {
    if d > 0.0 {
        d as i32
    } else {
        (d as i32).wrapping_sub(1)
    }
}

#[inline]
fn dot(g: &[i32; 3], x: f64, y: f64) -> f64 {
    g[0] as f64 * x + g[1] as f64 * y
}

impl NoiseGeneratorSimplex {
    pub fn new(rand: &mut JavaRandom) -> Self {
        let (x, y, z, p) = init_perm(rand);
        NoiseGeneratorSimplex {
            p,
            xo: x,
            yo: y,
            zo: z,
        }
    }

    #[inline]
    fn sample(&self, x: f64, y: f64, f2: f64, g2: f64) -> f64 {
        let p = &self.p;
        let s = (x + y) * f2;
        let i = fast_floor(x + s);
        let j = fast_floor(y + s);
        let t = (i.wrapping_add(j)) as f64 * g2;
        let x0o = i as f64 - t;
        let y0o = j as f64 - t;
        let x0 = x - x0o;
        let y0 = y - y0o;
        let (i1, j1) = if x0 > y0 { (1usize, 0usize) } else { (0, 1) };
        let x1 = x0 - i1 as f64 + g2;
        let y1 = y0 - j1 as f64 + g2;
        let x2 = x0 - 1.0 + 2.0 * g2;
        let y2 = y0 - 1.0 + 2.0 * g2;
        let ii = (i & 0xFF) as usize;
        let jj = (j & 0xFF) as usize;
        let gi0 = (p[ii + p[jj] as usize] % 12) as usize;
        let gi1 = (p[ii + i1 + p[jj + j1] as usize] % 12) as usize;
        let gi2 = (p[ii + 1 + p[jj + 1] as usize] % 12) as usize;
        let mut t0 = 0.5 - x0 * x0 - y0 * y0;
        let n0 = if t0 < 0.0 {
            0.0
        } else {
            t0 *= t0;
            t0 * t0 * dot(&GRAD3[gi0], x0, y0)
        };
        let mut t1 = 0.5 - x1 * x1 - y1 * y1;
        let n1 = if t1 < 0.0 {
            0.0
        } else {
            t1 *= t1;
            t1 * t1 * dot(&GRAD3[gi1], x1, y1)
        };
        let mut t2 = 0.5 - x2 * x2 - y2 * y2;
        let n2 = if t2 < 0.0 {
            0.0
        } else {
            t2 *= t2;
            t2 * t2 * dot(&GRAD3[gi2], x2, y2)
        };
        n0 + n1 + n2
    }

    pub fn get_value(&self, x: f64, y: f64) -> f64 {
        let sqrt3 = 3.0f64.sqrt();
        let f2 = 0.5 * (sqrt3 - 1.0);
        let g2 = (3.0 - sqrt3) / 6.0;
        70.0 * self.sample(x, y, f2, g2)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add(
        &self,
        arr: &mut [f64],
        x: f64,
        z: f64,
        xs: i32,
        zs: i32,
        sx: f64,
        sz: f64,
        amp: f64,
    ) {
        let sqrt3 = 3.0f64.sqrt();
        let f2 = 0.5 * (sqrt3 - 1.0);
        let g2 = (3.0 - sqrt3) / 6.0;
        let mut k = 0usize;
        for iz in 0..zs {
            let yy = (z + iz as f64) * sz + self.yo;
            for ix in 0..xs {
                let xx = (x + ix as f64) * sx + self.xo;
                arr[k] += 70.0 * self.sample(xx, yy, f2, g2) * amp;
                k += 1;
            }
        }
    }
}

pub struct NoiseGeneratorPerlin {
    levels: Vec<NoiseGeneratorSimplex>,
}

impl NoiseGeneratorPerlin {
    pub fn new(rand: &mut JavaRandom, levels: usize) -> Self {
        NoiseGeneratorPerlin {
            levels: (0..levels)
                .map(|_| NoiseGeneratorSimplex::new(rand))
                .collect(),
        }
    }

    pub fn get_value(&self, x: f64, z: f64) -> f64 {
        let mut v = 0.0;
        let mut f = 1.0;
        for l in &self.levels {
            v += l.get_value(x * f, z * f) / f;
            f /= 2.0;
        }
        v
    }

    /// `getRegion(arr, x, z, xs, zs, sx, sz, lacunarity)` with persistence 0.5.
    #[allow(clippy::too_many_arguments)]
    pub fn get_region(
        &self,
        arr: &mut [f64],
        x: f64,
        z: f64,
        xs: i32,
        zs: i32,
        sx: f64,
        sz: f64,
        lacunarity: f64,
    ) {
        for v in arr.iter_mut() {
            *v = 0.0;
        }
        let mut a = 1.0f64;
        let mut f = 1.0f64;
        for l in &self.levels {
            l.add(arr, x, z, xs, zs, sx * f * a, sz * f * a, 0.55 / a);
            f *= lacunarity;
            a *= 0.5;
        }
    }
}
