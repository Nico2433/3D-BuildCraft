//! The per-frame paint callback: uploads in `prepare`, draw order in `paint`.

use super::*;

impl CallbackTrait for FrameCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen: &egui_wgpu::ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(r) = resources.get_mut::<Renderer>() else {
            return vec![];
        };
        let f = |b: bool| if b { 1.0 } else { 0.0 };
        // From the front left, as in the original.
        let light = (Vec3::new(-0.35, -0.55, 0.75).normalize()).extend(0.0);
        let globals = Globals {
            view_proj: mat(self.view_proj),
            eye: self.eye.extend(1.0).to_array(),
            light: light.to_array(),
            flags: [
                f(self.flags.shading),
                f(self.flags.smoothing),
                f(self.flags.xray),
                f(self.flags.colors),
            ],
            flags2: [
                f(self.flags.floor),
                f(self.flags.reflections),
                self.flags.floor_tone as f32,
                0.0,
            ],
            viewport: [self.viewport.x, self.viewport.y, 0.0, 0.0],
            clip: self.clip.map_or([0.0; 4], |(n, d)| [n.x, n.y, n.z, d]),
            decal: mat(self.decal.map_or(Mat4::ZERO, |d| d.decal.shader_matrix())),
            decal_n: self.decal.map_or([0.0; 4], |d| {
                d.decal.normal.extend(d.decal.reach()).to_array()
            }),
            decal_p: [self.decal.map_or(0.0, |d| d.layer as f32), 0.0, 0.0, 0.0],
        };
        r.upload_textures(queue);
        queue.write_buffer(&r.globals_buf, 0, bytemuck::bytes_of(&globals));

        // Three slots per item: the object, its reflection and its shadow; then the slab.
        let n = self.items.len();
        if 3 * n as u64 + 1 > r.object_capacity {
            r.object_capacity = (3 * n as u64 + 1).next_power_of_two();
            let (b, bg) = Renderer::object_buffer(device, &r.object_layout, r.object_capacity);
            r.object_buf = b;
            r.object_bg = bg;
        }
        let mut bytes = std::mem::take(&mut r.object_bytes);
        bytes.clear();
        bytes.resize((3 * n + 1) * OBJECT_STRIDE as usize, 0);
        let mut live = HashSet::new();
        for (i, it) in self.items.iter().enumerate() {
            let key = Arc::as_ptr(&it.mesh) as usize;
            live.insert(key);
            let gm = r
                .meshes
                .entry(key)
                .or_insert_with(|| Renderer::upload_mesh(device, &it.mesh));
            let c = it.color;
            let color = Vec4::new(c[0] as f32, c[1] as f32, c[2] as f32, c[3] as f32)
                .to_array()
                .map(|x| x / 255.0);
            let mirrored = Mat4::from_scale(Vec3::new(1.0, 1.0, -1.0)) * it.model;
            // Transform the cached local box in constant time. A soft shadow needs
            // only a conservative footprint, not a scan of every rotated vertex.
            let local_half = gm.bounds.size() * 0.5;
            let extent = it.model.x_axis.truncate().abs() * local_half.x
                + it.model.y_axis.truncate().abs() * local_half.y
                + it.model.z_axis.truncate().abs() * local_half.z;
            let half = extent.truncate() * 1.2 + glam::Vec2::splat(2.0);
            let centre = it.model.transform_point3(gm.bounds.center()).truncate()
                - light.truncate().truncate() * 0.15 * half.length();
            let patch =
                Mat4::from_translation(centre.extend(0.01)) * Mat4::from_scale(half.extend(1.0));
            for (slot, model, pass) in [
                (i, it.model, 0.0),
                (n + i, mirrored, 1.0),
                (2 * n + i, patch, 2.0),
            ] {
                let u = ObjectUniform {
                    model: mat(model),
                    normal: mat(model.inverse().transpose()),
                    color,
                    extra: [
                        f(it.selected),
                        pass,
                        f(it.material == Material::Realistic),
                        f(self.decal.is_some_and(|d| d.item == i)),
                    ],
                };
                let o = slot * OBJECT_STRIDE as usize;
                bytes[o..o + std::mem::size_of::<ObjectUniform>()]
                    .copy_from_slice(bytemuck::bytes_of(&u));
            }

            if self.flags.wireframe && gm.lines.is_none() {
                let idx = Renderer::line_indices(&gm.triangles);
                let buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("mesh lines"),
                    contents: bytemuck::cast_slice(&idx),
                    usage: wgpu::BufferUsages::INDEX,
                });
                gm.lines = Some((buf, idx.len() as u32));
            }
        }
        if let Some((model, color)) = self.slab {
            let u = ObjectUniform {
                model: mat(model),
                normal: mat(Mat4::IDENTITY),
                color,
                extra: [0.0; 4],
            };
            let o = 3 * n * OBJECT_STRIDE as usize;
            bytes[o..o + std::mem::size_of::<ObjectUniform>()]
                .copy_from_slice(bytemuck::bytes_of(&u));
        }
        if !bytes.is_empty() {
            queue.write_buffer(&r.object_buf, 0, &bytes);
        }
        r.object_bytes = bytes;
        r.meshes.retain(|k, _| live.contains(k));
        r.update_selection(self, device, screen, encoder);
        vec![]
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &CallbackResources,
    ) {
        let Some(r) = resources.get::<Renderer>() else {
            return;
        };
        pass.set_bind_group(0, &r.globals_bg, &[]);
        // Textures for every solid/x-ray draw, whether or not the reflection runs.
        pass.set_bind_group(2, &r.texture_bg, &[]);
        let n = self.items.len() as u64;
        let slot = |s: u64| [(s * OBJECT_STRIDE) as u32];
        if self.flags.floor && self.flags.reflections && !self.flags.xray && self.eye.z > 0.0 {
            pass.set_pipeline(&r.solid);
            for (i, it) in self.items.iter().enumerate() {
                let Some(gm) = r.meshes.get(&(Arc::as_ptr(&it.mesh) as usize)) else {
                    continue;
                };
                if gm.index_count == 0 {
                    continue;
                }
                pass.set_bind_group(1, &r.object_bg, &slot(n + i as u64));
                pass.set_vertex_buffer(0, gm.vbuf.slice(..));
                pass.set_index_buffer(gm.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..gm.index_count, 0, 0..1);
            }
        }
        if self.flags.floor {
            pass.set_pipeline(&r.floor);
            pass.draw(0..6, 0..1);
        }
        if self.flags.floor && self.flags.shadows {
            pass.set_pipeline(&r.shadow);
            for i in 0..n {
                pass.set_bind_group(1, &r.object_bg, &slot(2 * n + i));
                pass.draw(0..6, 0..1);
            }
        }
        // Opaque objects first, then the translucent ones far to near, which blend over
        // them without writing depth.
        let draw_item = |pass: &mut wgpu::RenderPass<'static>, i: usize| {
            let it = &self.items[i];
            let Some(gm) = r.meshes.get(&(Arc::as_ptr(&it.mesh) as usize)) else {
                return;
            };
            if gm.index_count == 0 {
                return;
            }
            let see_through = self.flags.xray || it.material == Material::Translucent;
            pass.set_pipeline(if see_through { &r.xray } else { &r.solid });
            pass.set_bind_group(1, &r.object_bg, &[(i as u64 * OBJECT_STRIDE) as u32]);
            pass.set_vertex_buffer(0, gm.vbuf.slice(..));
            pass.set_index_buffer(gm.ibuf.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..gm.index_count, 0, 0..1);
        };
        let translucent = |i: &usize| self.items[*i].material == Material::Translucent;
        for i in (0..self.items.len()).filter(|i| !translucent(i)) {
            draw_item(pass, i);
        }
        // Empty (no allocation) unless something is translucent.
        let far = |i: usize| {
            self.eye
                .distance(self.items[i].model.transform_point3(Vec3::ZERO))
        };
        let mut glass: Vec<usize> = (0..self.items.len()).filter(translucent).collect();
        glass.sort_by(|&a, &b| far(b).total_cmp(&far(a)));
        for i in glass {
            draw_item(pass, i);
        }
        if self.slab.is_some() {
            pass.set_pipeline(&r.slab);
            pass.set_bind_group(1, &r.object_bg, &slot(3 * n));
            pass.draw(0..6, 0..1);
        }
        if self.flags.wireframe {
            pass.set_pipeline(&r.wire);
            // Not on see-through items: the wire belongs to what is being made.
            for (i, it) in self
                .items
                .iter()
                .enumerate()
                .filter(|(_, it)| it.color[3] == 255)
            {
                let Some(gm) = r.meshes.get(&(Arc::as_ptr(&it.mesh) as usize)) else {
                    continue;
                };
                let Some((lines, count)) = &gm.lines else {
                    continue;
                };
                if *count == 0 {
                    continue;
                }
                pass.set_bind_group(1, &r.object_bg, &[(i as u64 * OBJECT_STRIDE) as u32]);
                pass.set_vertex_buffer(0, gm.vbuf.slice(..));
                pass.set_index_buffer(lines.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..*count, 0, 0..1);
            }
        }
        if let Some(selection) = &r.selection {
            pass.set_pipeline(&r.outline);
            pass.set_bind_group(0, &selection.outline_bg, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}
