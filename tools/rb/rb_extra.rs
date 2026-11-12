
    fn reset_uv(&mut self) {
        self.uvRotateEast = 0;
        self.uvRotateWest = 0;
        self.uvRotateSouth = 0;
        self.uvRotateNorth = 0;
        self.uvRotateTop = 0;
        self.uvRotateBottom = 0;
    }

    /// `RenderBlocks.renderBlockLog` (render type 31: logs, hay)
    pub fn render_block_log(&mut self, x: i32, y: i32, z: i32) -> bool {
        let m = self.meta & 12;
        if m == 4 {
            self.uvRotateEast = 1;
            self.uvRotateWest = 1;
            self.uvRotateTop = 1;
            self.uvRotateBottom = 1;
        } else if m == 8 {
            self.uvRotateSouth = 1;
            self.uvRotateNorth = 1;
        }
        let r = self.render_standard(x, y, z);
        self.reset_uv();
        r
    }

    /// `RenderBlocks.renderBlockQuartz` (render type 39)
    pub fn render_block_quartz(&mut self, x: i32, y: i32, z: i32) -> bool {
        if self.meta == 3 {
            self.uvRotateEast = 1;
            self.uvRotateWest = 1;
            self.uvRotateTop = 1;
            self.uvRotateBottom = 1;
        } else if self.meta == 4 {
            self.uvRotateSouth = 1;
            self.uvRotateNorth = 1;
        }
        let r = self.render_standard(x, y, z);
        self.reset_uv();
        r
    }

    /// `RenderBlocks.renderPistonBase`; `all` is the extended look the
    /// moving-block renderer asks for.
    pub fn render_piston_base(&mut self, x: i32, y: i32, z: i32, all: bool) -> bool {
        let m = self.meta;
        let extended = all || m & 8 != 0;
        let o = m & 7;
        if extended {
            match o {
                0 => {
                    self.uvRotateEast = 3;
                    self.uvRotateWest = 3;
                    self.uvRotateSouth = 3;
                    self.uvRotateNorth = 3;
                    self.set_render_bounds(0.0, 0.25, 0.0, 1.0, 1.0, 1.0);
                }
                1 => self.set_render_bounds(0.0, 0.0, 0.0, 1.0, 0.75, 1.0),
                2 => {
                    self.uvRotateSouth = 1;
                    self.uvRotateNorth = 2;
                    self.set_render_bounds(0.0, 0.0, 0.25, 1.0, 1.0, 1.0);
                }
                3 => {
                    self.uvRotateSouth = 2;
                    self.uvRotateNorth = 1;
                    self.uvRotateTop = 3;
                    self.uvRotateBottom = 3;
                    self.set_render_bounds(0.0, 0.0, 0.0, 1.0, 1.0, 0.75);
                }
                4 => {
                    self.uvRotateEast = 1;
                    self.uvRotateWest = 2;
                    self.uvRotateTop = 2;
                    self.uvRotateBottom = 1;
                    self.set_render_bounds(0.25, 0.0, 0.0, 1.0, 1.0, 1.0);
                }
                5 => {
                    self.uvRotateEast = 2;
                    self.uvRotateWest = 1;
                    self.uvRotateTop = 1;
                    self.uvRotateBottom = 2;
                    self.set_render_bounds(0.0, 0.0, 0.0, 0.75, 1.0, 1.0);
                }
                _ => {}
            }
            // setShapeForRendering: the block bounds follow (as f32).
            let f = |v: f64| v as f32 as f64;
            let (a, b, c, d, e, g) = (
                f(self.renderMinX),
                f(self.renderMinY),
                f(self.renderMinZ),
                f(self.renderMaxX),
                f(self.renderMaxY),
                f(self.renderMaxZ),
            );
            self.set_render_bounds(a, b, c, d, e, g);
            self.render_standard(x, y, z);
            self.reset_uv();
            self.set_render_bounds(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
        } else {
            match o {
                0 => {
                    self.uvRotateEast = 3;
                    self.uvRotateWest = 3;
                    self.uvRotateSouth = 3;
                    self.uvRotateNorth = 3;
                }
                2 => {
                    self.uvRotateSouth = 1;
                    self.uvRotateNorth = 2;
                }
                3 => {
                    self.uvRotateSouth = 2;
                    self.uvRotateNorth = 1;
                    self.uvRotateTop = 3;
                    self.uvRotateBottom = 3;
                }
                4 => {
                    self.uvRotateEast = 1;
                    self.uvRotateWest = 2;
                    self.uvRotateTop = 2;
                    self.uvRotateBottom = 1;
                }
                5 => {
                    self.uvRotateEast = 2;
                    self.uvRotateWest = 1;
                    self.uvRotateTop = 1;
                    self.uvRotateBottom = 2;
                }
                _ => {}
            }
            self.render_standard(x, y, z);
            self.reset_uv();
        }
        true
    }

    fn rod_icon(&self) -> Icon {
        self.overrideBlockTexture
            .unwrap_or_else(|| Icon::new(self.atlas().named("piston_side")))
    }

    /// `RenderBlocks.renderPistonRodUD`
    #[allow(clippy::too_many_arguments)]
    fn rod_ud(&mut self, a: f64, b: f64, c: f64, d: f64, e: f64, f: f64, shade: f32, len: f64) {
        let i = self.rod_icon();
        let (u0, v0, u1, v1) = (i.min_u(), i.min_v(), i.u(len), i.v(4.0));
        self.color(shade, shade, shade);
        self.vertex(a, d, e, u1, v0);
        self.vertex(a, c, e, u0, v0);
        self.vertex(b, c, f, u0, v1);
        self.vertex(b, d, f, u1, v1);
    }

    /// `RenderBlocks.renderPistonRodSN`
    #[allow(clippy::too_many_arguments)]
    fn rod_sn(&mut self, a: f64, b: f64, c: f64, d: f64, e: f64, f: f64, shade: f32, len: f64) {
        let i = self.rod_icon();
        let (u0, v0, u1, v1) = (i.min_u(), i.min_v(), i.u(len), i.v(4.0));
        self.color(shade, shade, shade);
        self.vertex(a, c, f, u1, v0);
        self.vertex(a, c, e, u0, v0);
        self.vertex(b, d, e, u0, v1);
        self.vertex(b, d, f, u1, v1);
    }

    /// `RenderBlocks.renderPistonRodEW`
    #[allow(clippy::too_many_arguments)]
    fn rod_ew(&mut self, a: f64, b: f64, c: f64, d: f64, e: f64, f: f64, shade: f32, len: f64) {
        let i = self.rod_icon();
        let (u0, v0, u1, v1) = (i.min_u(), i.min_v(), i.u(len), i.v(4.0));
        self.color(shade, shade, shade);
        self.vertex(b, c, e, u1, v0);
        self.vertex(a, c, e, u0, v0);
        self.vertex(a, d, f, u0, v1);
        self.vertex(b, d, f, u1, v1);
    }

    /// `RenderBlocks.renderPistonExtension`; `long` gives the full-length arm.
    pub fn render_piston_extension(&mut self, x: i32, y: i32, z: i32, long: bool) -> bool {
        let o = self.meta & 7;
        let l = if long { 1.0f32 } else { 0.5f32 };
        let len = if long { 16.0 } else { 8.0 };
        // Java adds the float offsets to world coordinates.
        let (ox, oz) = (self.c().r.ox, self.c().r.oz);
        let (xf, yf, zf) = ((x + ox) as f32, y as f32, (z + oz) as f32);
        let p = |v: f32| v as f64;
        let px = |v: f32| v as f64 - ox as f64;
        let pz = |v: f32| v as f64 - oz as f64;
        match o {
            0 => {
                self.uvRotateEast = 3;
                self.uvRotateWest = 3;
                self.uvRotateSouth = 3;
                self.uvRotateNorth = 3;
                self.set_render_bounds(0.0, 0.0, 0.0, 1.0, 0.25, 1.0);
                self.render_standard(x, y, z);
                let (y0, y1) = (p(yf + 0.25), p(yf + 0.25 + l));
                self.rod_ud(px(xf + 0.375), px(xf + 0.625), y0, y1, pz(zf + 0.625), pz(zf + 0.625), 0.8, len);
                self.rod_ud(px(xf + 0.625), px(xf + 0.375), y0, y1, pz(zf + 0.375), pz(zf + 0.375), 0.8, len);
                self.rod_ud(px(xf + 0.375), px(xf + 0.375), y0, y1, pz(zf + 0.375), pz(zf + 0.625), 0.6, len);
                self.rod_ud(px(xf + 0.625), px(xf + 0.625), y0, y1, pz(zf + 0.625), pz(zf + 0.375), 0.6, len);
            }
            1 => {
                self.set_render_bounds(0.0, 0.75, 0.0, 1.0, 1.0, 1.0);
                self.render_standard(x, y, z);
                let (y0, y1) = (p(yf - 0.25 + 1.0 - l), p(yf - 0.25 + 1.0));
                self.rod_ud(px(xf + 0.375), px(xf + 0.625), y0, y1, pz(zf + 0.625), pz(zf + 0.625), 0.8, len);
                self.rod_ud(px(xf + 0.625), px(xf + 0.375), y0, y1, pz(zf + 0.375), pz(zf + 0.375), 0.8, len);
                self.rod_ud(px(xf + 0.375), px(xf + 0.375), y0, y1, pz(zf + 0.375), pz(zf + 0.625), 0.6, len);
                self.rod_ud(px(xf + 0.625), px(xf + 0.625), y0, y1, pz(zf + 0.625), pz(zf + 0.375), 0.6, len);
            }
            2 => {
                self.uvRotateSouth = 1;
                self.uvRotateNorth = 2;
                self.set_render_bounds(0.0, 0.0, 0.0, 1.0, 1.0, 0.25);
                self.render_standard(x, y, z);
                let (z0, z1) = (pz(zf + 0.25), pz(zf + 0.25 + l));
                self.rod_sn(px(xf + 0.375), px(xf + 0.375), p(yf + 0.625), p(yf + 0.375), z0, z1, 0.6, len);
                self.rod_sn(px(xf + 0.625), px(xf + 0.625), p(yf + 0.375), p(yf + 0.625), z0, z1, 0.6, len);
                self.rod_sn(px(xf + 0.375), px(xf + 0.625), p(yf + 0.375), p(yf + 0.375), z0, z1, 0.5, len);
                self.rod_sn(px(xf + 0.625), px(xf + 0.375), p(yf + 0.625), p(yf + 0.625), z0, z1, 1.0, len);
            }
            3 => {
                self.uvRotateSouth = 2;
                self.uvRotateNorth = 1;
                self.uvRotateTop = 3;
                self.uvRotateBottom = 3;
                self.set_render_bounds(0.0, 0.0, 0.75, 1.0, 1.0, 1.0);
                self.render_standard(x, y, z);
                let (z0, z1) = (pz(zf - 0.25 + 1.0 - l), pz(zf - 0.25 + 1.0));
                self.rod_sn(px(xf + 0.375), px(xf + 0.375), p(yf + 0.625), p(yf + 0.375), z0, z1, 0.6, len);
                self.rod_sn(px(xf + 0.625), px(xf + 0.625), p(yf + 0.375), p(yf + 0.625), z0, z1, 0.6, len);
                self.rod_sn(px(xf + 0.375), px(xf + 0.625), p(yf + 0.375), p(yf + 0.375), z0, z1, 0.5, len);
                self.rod_sn(px(xf + 0.625), px(xf + 0.375), p(yf + 0.625), p(yf + 0.625), z0, z1, 1.0, len);
            }
            4 => {
                self.uvRotateEast = 1;
                self.uvRotateWest = 2;
                self.uvRotateTop = 2;
                self.uvRotateBottom = 1;
                self.set_render_bounds(0.0, 0.0, 0.0, 0.25, 1.0, 1.0);
                self.render_standard(x, y, z);
                let (x0, x1) = (px(xf + 0.25), px(xf + 0.25 + l));
                self.rod_ew(x0, x1, p(yf + 0.375), p(yf + 0.375), pz(zf + 0.625), pz(zf + 0.375), 0.5, len);
                self.rod_ew(x0, x1, p(yf + 0.625), p(yf + 0.625), pz(zf + 0.375), pz(zf + 0.625), 1.0, len);
                self.rod_ew(x0, x1, p(yf + 0.375), p(yf + 0.625), pz(zf + 0.375), pz(zf + 0.375), 0.6, len);
                self.rod_ew(x0, x1, p(yf + 0.625), p(yf + 0.375), pz(zf + 0.625), pz(zf + 0.625), 0.6, len);
            }
            5 => {
                self.uvRotateEast = 2;
                self.uvRotateWest = 1;
                self.uvRotateTop = 1;
                self.uvRotateBottom = 2;
                self.set_render_bounds(0.75, 0.0, 0.0, 1.0, 1.0, 1.0);
                self.render_standard(x, y, z);
                let (x0, x1) = (px(xf - 0.25 + 1.0 - l), px(xf - 0.25 + 1.0));
                self.rod_ew(x0, x1, p(yf + 0.375), p(yf + 0.375), pz(zf + 0.625), pz(zf + 0.375), 0.5, len);
                self.rod_ew(x0, x1, p(yf + 0.625), p(yf + 0.625), pz(zf + 0.375), pz(zf + 0.625), 1.0, len);
                self.rod_ew(x0, x1, p(yf + 0.375), p(yf + 0.625), pz(zf + 0.375), pz(zf + 0.375), 0.6, len);
                self.rod_ew(x0, x1, p(yf + 0.625), p(yf + 0.375), pz(zf + 0.625), pz(zf + 0.625), 0.6, len);
            }
            _ => {}
        }
        self.reset_uv();
        self.set_render_bounds(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
        true
    }

    /// `RenderBlocks.renderBlockSandFalling`: the block centred on the
    /// origin, flat shaded, lit with one brightness.
    pub fn render_sand_falling(&mut self, bright: i32) {
        self.set_render_bounds(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
        self.bright = bright;
        self.color(0.5, 0.5, 0.5);
        let i = self.side_icon(0);
        self.face_y_neg(-0.5, -0.5, -0.5, i);
        self.color(1.0, 1.0, 1.0);
        let i = self.side_icon(1);
        self.face_y_pos(-0.5, -0.5, -0.5, i);
        self.color(0.8, 0.8, 0.8);
        let i = self.side_icon(2);
        self.face_z_neg(-0.5, -0.5, -0.5, i);
        self.color(0.8, 0.8, 0.8);
        let i = self.side_icon(3);
        self.face_z_pos(-0.5, -0.5, -0.5, i);
        self.color(0.6, 0.6, 0.6);
        let i = self.side_icon(4);
        self.face_x_neg(-0.5, -0.5, -0.5, i);
        self.color(0.6, 0.6, 0.6);
        let i = self.side_icon(5);
        self.face_x_pos(-0.5, -0.5, -0.5, i);
    }
